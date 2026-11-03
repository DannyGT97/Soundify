import { beforeEach, describe, expect, it, vi } from 'vitest';

// The panel's side of the Java UpdatesSettingsController: what the button and
// the status line say through a check and an install. The back is faked.

const api = vi.hoisted(() => ({
  checkAppUpdate: vi.fn(),
  installAppUpdate: vi.fn(),
  onAppUpdate: vi.fn(),
}));
const notify = vi.hoisted(() => vi.fn());
const openUpdateAvailable = vi.hoisted(() => vi.fn());

vi.mock('./api', () => ({ api }));
vi.mock('./appInfo', () => ({ appVersion: () => 'v1.0.0' }));
vi.mock('./components/toasts', () => ({ notify }));
vi.mock('./overlays/updateAvailable', () => ({ openUpdateAvailable }));

const { checkForUpdate, progressText, updateState } = await import('./appUpdate');

const UPDATE = { version: 'v1.1.0', notes: '' };

beforeEach(() => {
  vi.clearAllMocks();
});

describe('progressText', () => {
  it('says each phase with its version and percent', () => {
    expect(progressText({ phase: 'downloading', version: 'v1.1.0', percent: 42 })).toContain('42%');
    expect(progressText({ phase: 'verified' })).toContain('verificada');
    expect(progressText({ phase: 'installing', version: 'v1.1.0' })).toContain('v1.1.0');
  });
});

describe('checkForUpdate', () => {
  it('nothing new: a green toast with the version, and the button back to normal', async () => {
    api.checkAppUpdate.mockResolvedValue(null);

    await checkForUpdate();

    expect(notify).toHaveBeenCalledWith({ tone: 'success', text: expect.stringContaining('v1.0.0') });
    expect(updateState()).toMatchObject({ pending: null, busy: false, status: '', failed: false });
    expect(openUpdateAvailable).not.toHaveBeenCalled();
  });

  it('a failed check is said in red and the button offers to try again', async () => {
    api.checkAppUpdate.mockRejectedValue({ key: 'update.badHashFile', args: [] });

    await checkForUpdate();

    expect(notify).toHaveBeenCalledWith({ tone: 'error', text: expect.stringContaining('SHA-256') });
    expect(updateState()).toMatchObject({ busy: false, failed: true });
  });

  it('a new version opens the window and the next click installs it', async () => {
    api.checkAppUpdate.mockResolvedValue(UPDATE);
    await checkForUpdate();
    expect(openUpdateAvailable).toHaveBeenCalledWith(UPDATE, 'v1.0.0', expect.any(Function));
    expect(updateState().pending).toEqual(UPDATE);

    const statuses: string[] = [];
    api.installAppUpdate.mockImplementation(async (onProgress: (p: unknown) => void) => {
      onProgress({ phase: 'downloading', version: 'v1.1.0', percent: 50 });
      statuses.push(updateState().status);
      onProgress({ phase: 'installing', version: 'v1.1.0' });
      statuses.push(updateState().status);
    });
    await checkForUpdate();

    expect(api.checkAppUpdate).toHaveBeenCalledTimes(1);
    expect(statuses[0]).toContain('50%');
    expect(statuses[1]).toContain('Instalando');
    // Until the app closes the button stays off: a second click must not download twice.
    expect(updateState().busy).toBe(true);
  });

  it('security: a rejected download installs nothing and forgets the version', async () => {
    // The previous test left it busy installing; start from a found update.
    Object.assign(updateState(), { busy: false, pending: UPDATE });
    api.installAppUpdate.mockRejectedValue({ key: 'update.signatureInvalid', args: [] });

    await checkForUpdate();

    expect(notify).toHaveBeenCalledWith({ tone: 'error', text: expect.stringContaining('No se ha instalado nada') });
    expect(updateState()).toMatchObject({ pending: null, busy: false, failed: true });
  });
});
