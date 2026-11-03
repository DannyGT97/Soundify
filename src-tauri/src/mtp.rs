//! The phone transport (= Java `WindowsPortableDevice` + `PortableDevices`):
//! the Windows implementation of `PortableDevice`, and `NoDevices` anywhere
//! else.
//!
//! Two COM techniques, the same ones the Java app verified against real
//! phones: `Shell.Application` (its `Folder`/`FolderItem` objects) to list,
//! create folders and navigate, and `IFileOperation` to copy (both ways) and
//! delete.
//!
//! Copying used to go through `Folder.CopyHere`: it works, but it goes through
//! `wpdshext.dll`, the MTP shell extension, which pops its OWN "Copiando..."
//! dialog on every copy REGARDLESS of `FOF_SILENT` — a limitation of that
//! extension, not a missing flag (the very same flags already silence
//! `IFileOperation::DeleteItem`). `IFileOperation::CopyItem` does not carry
//! that dialog, and `PerformOperations()` is synchronous: when it returns the
//! copy is done or it failed, no polling.
//!
//! Every call runs on a fresh thread with its own COM apartment and a bound:
//! a device call can hang for a long time with nothing on this side able to
//! interrupt it, so the caller stops waiting after a while and treats it as a
//! failure (the abandoned thread finishes, or not, on its own).

use std::sync::Arc;

#[cfg(not(windows))]
use soundify_core::device::NoDevices;
use soundify_core::device::PortableDevice;

/// The transport for this OS.
pub fn current() -> Arc<dyn PortableDevice> {
  #[cfg(windows)]
  {
    Arc::new(windows_impl::WindowsDevice)
  }
  #[cfg(not(windows))]
  {
    Arc::new(NoDevices)
  }
}

#[cfg(windows)]
mod windows_impl {
  use std::path::Path;
  use std::sync::mpsc;
  use std::time::Duration;

  use log::warn;
  use soundify_core::device::{DeviceEntry, DeviceInfo, PortableDevice};
  use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
  };
  use windows::Win32::System::Variant::VARIANT;
  use windows::Win32::UI::Shell::{
    FILEOPERATION_FLAGS, FOF_NOCONFIRMATION, FOF_NOCONFIRMMKDIR, FOF_NOERRORUI, FOF_SILENT, FileOperation, Folder,
    FolderItem, FolderItem2, IFileOperation, IFileOperationProgressSink, IShellDispatch, IShellItem,
    SHCreateItemFromIDList, SHGetIDListFromObject, Shell,
  };
  use windows::core::{BSTR, HSTRING, IUnknown, Interface, PCWSTR, Result};

  /// "This PC" for `Shell.NameSpace`.
  const SSF_DRIVES: i32 = 17;

  /// How long each kind of call is waited for.
  const CONNECTIVITY: Duration = Duration::from_secs(15);
  const BROWSE: Duration = Duration::from_secs(60);
  const TRANSFER: Duration = Duration::from_secs(90);

  pub struct WindowsDevice;

  impl PortableDevice for WindowsDevice {
    fn is_available(&self) -> bool {
      true
    }

    fn list_devices(&self) -> Vec<DeviceInfo> {
      bounded("list devices", CONNECTIVITY, Vec::new(), list_devices)
    }

    fn is_reachable(&self, device_id: &str) -> bool {
      let id = device_id.to_string();
      // The same storage-root resolution every real operation does —
      // deliberately NOT just finding the name in "This PC" (all that
      // list_devices() does): a charge-only phone can still show up there by
      // name while its folder has no storage inside, or throws, because no
      // MTP session was ever granted.
      bounded("check device", CONNECTIVITY, false, move || Ok(storage_root(&shell()?, &id)?.is_some()))
    }

    fn list(&self, device_id: &str, path: &str) -> Vec<DeviceEntry> {
      let (id, path) = (device_id.to_string(), path.to_string());
      bounded("list", BROWSE, Vec::new(), move || {
        let Some(root) = storage_root(&shell()?, &id)? else { return Ok(Vec::new()) };
        let Some(folder) = navigate(&root, &path, false)? else { return Ok(Vec::new()) };
        let items = unsafe { folder.Items()? };
        let mut result = Vec::new();
        for i in 0..unsafe { items.Count()? } {
          let item = unsafe { items.Item(&VARIANT::from(i))? };
          let is_folder = is_folder(&item)?;
          let size_bytes = if is_folder { 0 } else { extended_size(&item).unwrap_or(0) };
          result.push(DeviceEntry { name: name_of(&item)?, size_bytes, is_folder });
        }
        Ok(result)
      })
    }

    fn mkdirs(&self, device_id: &str, path: &str) -> bool {
      let (id, path) = (device_id.to_string(), path.to_string());
      bounded("create folder", BROWSE, false, move || {
        let Some(root) = storage_root(&shell()?, &id)? else { return Ok(false) };
        Ok(navigate(&root, &path, true)?.is_some())
      })
    }

    fn push(&self, device_id: &str, path: &str, local: &Path, dest_name: Option<&str>) -> bool {
      if !local.is_file() {
        return false;
      }
      let (id, path, local) = (device_id.to_string(), path.to_string(), local.to_path_buf());
      let dest_name = dest_name.filter(|n| !n.trim().is_empty()).map(str::to_string);
      bounded("push", TRANSFER, false, move || {
        let shell = shell()?;
        let Some(root) = storage_root(&shell, &id)? else { return Ok(false) };
        let Some(dest_folder) = navigate(&root, &path, true)? else { return Ok(false) };
        // navigate() hands back the FOLDER (its Items() verify the arrival
        // below); CopyItem wants the folder's own ITEM instead.
        let Some(dest_item) = resolve_item(&root, &path)? else { return Ok(false) };
        let local_name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(parent) = local.parent() else { return Ok(false) };
        // A real FolderItem, not a bare path string: a bare string silently
        // does nothing against a device.
        let src_folder = unsafe { shell.NameSpace(&VARIANT::from(parent.to_string_lossy().as_ref()))? };
        let Some(src_item) = find_child(&src_folder, &local_name)? else { return Ok(false) };
        let target = dest_name.unwrap_or_else(|| local_name.clone());
        // No copy name lands it under the source's own name; a real one lands
        // it under THAT one: the device shows "Title.mp3" while the staging
        // copy stays "<id>.<ext>" for its own recovery.
        let copy_name = (target != local_name).then(|| HSTRING::from(target.as_str()));
        let op = file_operation()?;
        unsafe {
          op.CopyItem(
            &shell_item(&src_item)?,
            &shell_item(&dest_item)?,
            copy_name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
            None::<&IFileOperationProgressSink>,
          )?;
          op.PerformOperations()?;
        }
        // Synchronous, so the copy is done or failed by now; still checked by
        // size, the same paranoia as before: a copy reported done that landed
        // short or empty is worse than one flagged as failed.
        let expected = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(u64::MAX);
        Ok(find_child(&dest_folder, &target)?.is_some_and(|found| extended_size(&found) == Some(expected)))
      })
    }

    fn delete(&self, device_id: &str, paths: &[String]) -> bool {
      if paths.is_empty() {
        return false;
      }
      let (id, paths) = (device_id.to_string(), paths.to_vec());
      bounded("delete", TRANSFER, false, move || {
        let Some(root) = storage_root(&shell()?, &id)? else { return Ok(false) };
        let op = file_operation()?;
        // One PerformOperations() for the whole batch: that is what makes it
        // cheap (50 files in about 2 s, not one round trip per file).
        let mut queued = 0;
        for path in &paths {
          let Some(item) = resolve_item(&root, path)? else {
            warn!("Not found on device, skipped: {id}:{path}");
            continue;
          };
          unsafe { op.DeleteItem(&shell_item(&item)?, None::<&IFileOperationProgressSink>)? };
          queued += 1;
        }
        if queued == 0 {
          return Ok(false);
        }
        unsafe { op.PerformOperations()? };
        Ok(true)
      })
    }

    fn pull(&self, device_id: &str, path: &str, local: &Path) -> bool {
      let (id, path, local) = (device_id.to_string(), path.to_string(), local.to_path_buf());
      bounded("pull", TRANSFER, false, move || {
        let shell = shell()?;
        let Some(root) = storage_root(&shell, &id)? else { return Ok(false) };
        let Some(src_item) = resolve_item(&root, &path)? else { return Ok(false) };
        let src_name = name_of(&src_item)?;
        let expected = extended_size(&src_item);
        let Some(dest_dir) = local.parent() else { return Ok(false) };
        if std::fs::create_dir_all(dest_dir).is_err() {
          return Ok(false);
        }
        // A stale file from an earlier failed pull would fool the arrival
        // check below (it may already have the right name): start clean.
        let landed = dest_dir.join(&src_name);
        let _ = std::fs::remove_file(&landed);
        // CopyItem always lands it under ITS OWN name; renamed below if the
        // caller asked for another one.
        let dest_folder = unsafe { shell.NameSpace(&VARIANT::from(dest_dir.to_string_lossy().as_ref()))? };
        let op = file_operation()?;
        unsafe {
          op.CopyItem(&shell_item(&src_item)?, &shell_item(&dest_folder)?, PCWSTR::null(), None::<&IFileOperationProgressSink>)?;
          op.PerformOperations()?;
        }
        let arrived = std::fs::metadata(&landed).ok().filter(|m| m.is_file()).map(|m| m.len());
        if arrived.is_none() || arrived != expected {
          return Ok(false);
        }
        if landed == local {
          return Ok(true);
        }
        let _ = std::fs::remove_file(&local);
        Ok(std::fs::rename(&landed, &local).is_ok())
      })
    }

    fn open_in_explorer(&self, device_id: &str, path: &str) -> bool {
      if path.trim().is_empty() {
        return false;
      }
      let (id, path) = (device_id.to_string(), path.to_string());
      bounded("open", CONNECTIVITY, false, move || {
        let Some(root) = storage_root(&shell()?, &id)? else { return Ok(false) };
        let Some(target) = resolve_item(&root, &path)? else { return Ok(false) };
        // The "open" verb on the folder's own item, what a double click does:
        // the only reliable way (a friendly "This PC\..." path handed to
        // explorer.exe does NOT work, tried repeatedly).
        unsafe { target.InvokeVerb(&VARIANT::from("open"))? };
        Ok(true)
      })
    }
  }

  // === THREADING ===

  /// Runs `work` on a fresh thread with its own COM apartment, waiting at most
  /// `limit`; a failure, a panic or the limit all give `fallback`.
  fn bounded<T: Send + 'static>(
    what: &'static str,
    limit: Duration,
    fallback: T,
    work: impl FnOnce() -> Result<T> + Send + 'static,
  ) -> T {
    let (sender, receiver) = mpsc::channel();
    let spawned = std::thread::Builder::new().name(format!("device-{what}")).spawn(move || {
      let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
      let result = work();
      // Released before the apartment closes: every COM object is gone by now.
      if initialized {
        unsafe { CoUninitialize() };
      }
      let _ = sender.send(result);
    });
    if let Err(e) = spawned {
      warn!("Device call \"{what}\" could not start: {e}");
      return fallback;
    }
    match receiver.recv_timeout(limit) {
      Ok(Ok(value)) => value,
      Ok(Err(e)) => {
        warn!("Device call \"{what}\" failed: {e}");
        fallback
      }
      Err(_) => {
        warn!("Device call \"{what}\" gave no answer in {} s", limit.as_secs());
        fallback
      }
    }
  }

  // === NAVIGATION ===

  fn shell() -> Result<IShellDispatch> {
    unsafe { CoCreateInstance(&Shell, None, CLSCTX_ALL) }
  }

  fn list_devices() -> Result<Vec<DeviceInfo>> {
    let this_pc = unsafe { shell()?.NameSpace(&VARIANT::from(SSF_DRIVES))? };
    let items = unsafe { this_pc.Items()? };
    let mut result = Vec::new();
    for i in 0..unsafe { items.Count()? } {
      let item = unsafe { items.Item(&VARIANT::from(i))? };
      // A portable device shows up under "This PC" as a folder with no real
      // file system underneath: that is what tells it apart from a drive.
      if is_folder(&item)? && !bool::from(unsafe { item.IsFileSystem()? }) {
        let name = name_of(&item)?;
        result.push(DeviceInfo { id: name.clone(), name });
      }
    }
    Ok(result)
  }

  /// "This PC" -> the device -> its (only) storage folder.
  fn storage_root(shell: &IShellDispatch, device_id: &str) -> Result<Option<Folder>> {
    let this_pc = unsafe { shell.NameSpace(&VARIANT::from(SSF_DRIVES))? };
    let Some(device) = find_child(&this_pc, device_id)? else { return Ok(None) };
    let device_folder = folder_of(&device)?;
    let items = unsafe { device_folder.Items()? };
    for i in 0..unsafe { items.Count()? } {
      let candidate = unsafe { items.Item(&VARIANT::from(i))? };
      if is_folder(&candidate)? {
        return Ok(Some(folder_of(&candidate)?));
      }
    }
    Ok(None)
  }

  /// Walks a "/" path from `from`; with `create`, makes whatever segment is missing.
  fn navigate(from: &Folder, path: &str, create: bool) -> Result<Option<Folder>> {
    let mut current = from.clone();
    for segment in path.split('/').filter(|s| !s.trim().is_empty()) {
      let mut child = find_child(&current, segment)?;
      if child.is_none() && create {
        unsafe { current.NewFolder(&BSTR::from(segment), &VARIANT::default())? };
        std::thread::sleep(Duration::from_millis(300));
        child = find_child(&current, segment)?;
      }
      let Some(child) = child else { return Ok(None) };
      current = folder_of(&child)?;
    }
    Ok(Some(current))
  }

  /// The item at the end of a "/" path — the item itself, not its folder.
  fn resolve_item(from: &Folder, path: &str) -> Result<Option<FolderItem>> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.trim().is_empty()).collect();
    let Some((last, parents)) = segments.split_last() else { return Ok(None) };
    let Some(parent) = navigate(from, &parents.join("/"), false)? else { return Ok(None) };
    find_child(&parent, last)
  }

  fn find_child(folder: &Folder, name: &str) -> Result<Option<FolderItem>> {
    let items = unsafe { folder.Items()? };
    for i in 0..unsafe { items.Count()? } {
      let candidate = unsafe { items.Item(&VARIANT::from(i))? };
      if name_of(&candidate)? == name {
        return Ok(Some(candidate));
      }
    }
    Ok(None)
  }

  fn folder_of(item: &FolderItem) -> Result<Folder> {
    unsafe { item.GetFolder()? }.cast()
  }

  fn name_of(item: &FolderItem) -> Result<String> {
    Ok(unsafe { item.Name()? }.to_string())
  }

  fn is_folder(item: &FolderItem) -> Result<bool> {
    Ok(unsafe { item.IsFolder()? }.into())
  }

  fn extended_size(item: &FolderItem) -> Option<u64> {
    let item: FolderItem2 = item.cast().ok()?;
    let value = unsafe { item.ExtendedProperty(&BSTR::from("System.Size")) }.ok()?;
    u64::try_from(&value).ok().or_else(|| value.to_string().trim().parse().ok())
  }

  // === COM PLUMBING ===

  /// FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR: the
  /// combination that leaves no dialog on screen.
  fn file_operation() -> Result<IFileOperation> {
    let op: IFileOperation = unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL)? };
    let flags = FOF_SILENT.0 | FOF_NOCONFIRMATION.0 | FOF_NOERRORUI.0 | FOF_NOCONFIRMMKDIR.0;
    unsafe { op.SetOperationFlags(FILEOPERATION_FLAGS(flags))? };
    Ok(op)
  }

  /// The modern `IShellItem` an automation `FolderItem` or `Folder` stands
  /// for: what `IFileOperation` wants, which does not speak the old objects.
  fn shell_item(object: &impl Interface) -> Result<IShellItem> {
    let unknown: IUnknown = object.cast()?;
    unsafe {
      let pidl = SHGetIDListFromObject(&unknown)?;
      let item = SHCreateItemFromIDList(pidl);
      CoTaskMemFree(Some(pidl as _));
      item
    }
  }
}

#[cfg(all(test, windows))]
mod tests {
  use super::*;

  /// Read-only: asks Windows which portable devices it sees. Run by hand
  /// (`cargo test -- --ignored`) to check the COM plumbing on a real machine.
  #[test]
  #[ignore]
  fn lists_the_devices_this_machine_sees() {
    let phone = current();
    let devices = phone.list_devices();
    for device in &devices {
      println!("{} reachable={}", device.name, phone.is_reachable(&device.id));
    }
    assert!(phone.list("no such device", "").is_empty());
  }
}
