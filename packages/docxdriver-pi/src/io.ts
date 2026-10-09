import { createHash, randomBytes } from 'node:crypto';
import { mkdir, open, realpath, rename, stat, unlink } from 'node:fs/promises';
import { basename, dirname, isAbsolute, join, normalize, resolve, sep } from 'node:path';

/**
 * Path and write helpers for the DOCX tools.
 *
 * Trust model: Node exposes no descriptor-relative filesystem operations —
 * there is no openat/renameat and FileHandle has no rename — so an
 * adversarial process could swap namespace entries between the repeated
 * realpath/hash checks here and the rename(2) that replaces the target. The
 * workspace filesystem is therefore trusted against adversarial
 * same-instant namespace swaps: the rechecks close every race an ordinary
 * concurrent writer can produce, and the host additionally revalidates the
 * receipt-bound canonical path identity (realpath equality) immediately
 * before the write and again inside `writeDocxBytes` right before rename.
 */
function inside(root: string, target: string): boolean { return target === root || target.startsWith(root + sep); }

/** Resolve lexical and symlink paths beneath the real cwd. */
export async function resolveUnderCwd(cwd: string, path: string): Promise<string> {
  if (typeof path !== 'string' || path.trim() === '') throw new Error('path must be a non-empty string');
  const root = await realpath(resolve(cwd));
  const candidate = normalize(isAbsolute(path) ? resolve(path) : join(root, path));
  if (!inside(root, candidate)) throw new Error(`path escapes cwd: ${path}`);
  const tail: string[] = [];
  let probe = candidate;
  for (;;) {
    try {
      const actual = await realpath(probe);
      if (!inside(root, actual)) throw new Error(`path escapes cwd: ${path}`);
      return tail.length ? normalize(join(actual, ...tail)) : actual;
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== 'ENOENT') throw e;
      const parent = dirname(probe);
      if (parent === probe) throw new Error(`path escapes cwd: ${path}`);
      tail.unshift(basename(probe)); probe = parent;
    }
  }
}

/** Format a byte count for diagnostics: 1024 -> '1 KiB', 16 MiB -> '16 MiB'. */
export function formatBytes(bytes: number): string {
  if (bytes % (1024 * 1024) === 0) return `${bytes / (1024 * 1024)} MiB`;
  if (bytes % 1024 === 0) return `${bytes / 1024} KiB`;
  return `${bytes} bytes`;
}

/** The document exceeds the host's DOCX input byte limit. */
export class DocumentTooLargeError extends Error {
  readonly code = 'document_too_large';
  constructor(readonly maxBytes: number, readonly actualBytes: number) {
    super(`document exceeds input limit (${formatBytes(maxBytes)})`);
    this.name = 'DocumentTooLargeError';
  }
}

/**
 * Stat-first, byte-capped, cooperatively abortable file read. The size is
 * checked BEFORE any allocation (a larger file is rejected with
 * `DocumentTooLargeError` without ever being read), and the read itself
 * listens for `signal`: an abort closes the handle — unblocking the pending
 * read — and rejects with the stable `Error('aborted')`. The stat probe
 * itself is not abortable (`fs.promises.stat` takes no signal); the abort
 * applies before allocation and during the read, which is the part that can
 * hang.
 */
export async function readCappedBytes(abs: string, maxBytes?: number, signal?: AbortSignal): Promise<Uint8Array> {
  if (signal?.aborted) throw new Error('aborted');
  const st = await stat(abs);
  if (maxBytes !== undefined && st.size > maxBytes) throw new DocumentTooLargeError(maxBytes, st.size);
  const handle = await open(abs, 'r');
  try {
    return await new Promise<Uint8Array>((resolveRead, reject) => {
      // Re-check after the async open: the signal may have fired while the
      // handle was being opened (an already-aborted signal never re-fires
      // its listeners).
      if (signal?.aborted) {
        handle.close().catch(() => undefined);
        reject(new Error('aborted'));
        return;
      }
      let settled = false;
      const onAbort = (): void => {
        if (settled) return;
        settled = true;
        // Closing the handle makes the pending read reject (EBADF); the
        // caller-visible rejection is the stable `aborted` error.
        handle.close().catch(() => undefined);
        reject(new Error('aborted'));
      };
      signal?.addEventListener('abort', onAbort, { once: true });
      handle.read(Buffer.allocUnsafe(st.size), 0, st.size, 0).then(
        ({ bytesRead, buffer }) => {
          if (settled) return;
          settled = true;
          signal?.removeEventListener('abort', onAbort);
          resolveRead(new Uint8Array(buffer.subarray(0, bytesRead)));
        },
        (error) => {
          if (settled) return;
          settled = true;
          signal?.removeEventListener('abort', onAbort);
          reject(error);
        },
      );
    });
  } finally {
    await handle.close().catch(() => undefined);
  }
}

/**
 * Resolve a path under cwd, read it, and hash it. When `maxBytes` is given,
 * a larger document is rejected with `DocumentTooLargeError` BEFORE any
 * allocation, rendering, or engine call (the host passes its `maxDocxBytes`
 * limit; the read is stat-first and capped).
 */
export async function readDocxBytes(cwd: string, path: string, maxBytes?: number): Promise<{ abs: string; bytes: Uint8Array; sha256: string }> {
  const abs = await resolveUnderCwd(cwd, path);
  const bytes = await readCappedBytes(abs, maxBytes);
  return { abs, bytes, sha256: sha256(bytes) };
}

export function sha256(bytes: Uint8Array): string { return createHash('sha256').update(bytes).digest('hex'); }

async function revalidateParent(root: string, abs: string, expectedParent?: string): Promise<string> {
  const parent = dirname(abs);
  const actual = await realpath(parent);
  if (!inside(root, actual)) throw new Error('path changed outside cwd before write');
  if (expectedParent !== undefined && actual !== expectedParent) throw new Error('path changed outside cwd before write');
  return actual;
}

async function revalidateExisting(root: string, abs: string, expectedParent: string): Promise<void> {
  const parent = await revalidateParent(root, abs, expectedParent);
  const actual = await realpath(abs);
  if (!inside(root, actual) || !inside(parent, actual)) throw new Error('destination changed outside cwd before write');
}

async function revalidateAbsent(root: string, abs: string, expectedParent: string): Promise<void> {
  const parent = await revalidateParent(root, abs, expectedParent);
  try {
    const actual = await realpath(abs);
    if (!inside(root, actual) || !inside(parent, actual)) throw new Error('destination changed outside cwd before write');
    throw new Error('EEXIST: destination appeared before exclusive create');
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
  }
}

async function assertExpectedSource(abs: string, expectedSource: string, maxBytes?: number): Promise<void> {
  let current: Uint8Array;
  try {
    current = await readCappedBytes(abs, maxBytes);
  } catch (error) {
    if (error instanceof DocumentTooLargeError) {
      // The reviewed source was at most `maxBytes`; a larger file now is a
      // different source, never an infrastructure error.
      throw new SourceChangedError(expectedSource, `sha256:<oversize:${error.actualBytes} bytes>`);
    }
    throw error;
  }
  const actual = sha256(current);
  if (actual !== expectedSource) throw new SourceChangedError(expectedSource, actual);
}

/**
 * Best-effort fsync of the containing directory after rename. rename(2) is
 * atomic, but the directory entry itself must reach disk for the
 * replacement to be durable. fsync on an O_RDONLY directory fd works on
 * Linux; macOS returns EINVAL (APFS journals rename metadata), so that and
 * the other unsupported codes are swallowed while real errors propagate.
 */
export async function fsyncDirIfSupported(dir: string): Promise<void> {
  let fd;
  try {
    fd = await open(dir, 'r');
    await fd.sync();
  } catch (error) {
    const err = error as NodeJS.ErrnoException;
    if (err.code === 'EINVAL' || err.code === 'ENOTSUP' || err.code === 'EBADF' || err.code === 'EISDIR') return;
    throw error;
  } finally {
    if (fd) await fd.close().catch(() => undefined);
  }
}

/** The bytes read back after rename do not equal the verified candidate hash. */
export class ReadBackMismatchError extends Error {
  readonly code = 'readback_mismatch';
  constructor(readonly expected: string, readonly actual: string) {
    super(`read-back mismatch: expected sha256:${expected}, read back sha256:${actual}`);
    this.name = 'ReadBackMismatchError';
  }
}

/** The destination no longer resolves to the receipt-bound canonical path. */
export class PathChangedError extends Error {
  readonly code = 'path_changed';
  constructor(readonly expected: string, readonly actual: string) {
    super(`destination path changed before write: expected ${expected}, found ${actual}`);
    this.name = 'PathChangedError';
  }
}

/** Optional custom reader for the post-write read-back (test seam). */
export type ReadBack = (abs: string) => Promise<Uint8Array>;

/**
 * Read the file back after rename and assert its sha256 equals the verified
 * candidate hash exactly. The default read is stat-first and capped at the
 * exact expected size (a real pre-allocation bound — the verified
 * candidate's byte length), so an oversized or undersized read-back is a
 * mismatch without any full-file read. A mismatch is an infrastructure
 * error — the mutation has already occurred and the caller decides receipt
 * fate; it is never a success notice.
 */
export async function readBackVerify(abs: string, expectedSha256: string, expectedSize: number, readBack?: ReadBack): Promise<void> {
  let written: Uint8Array;
  if (readBack !== undefined) {
    written = new Uint8Array(await readBack(abs));
  } else {
    const st = await stat(abs);
    if (st.size !== expectedSize) throw new ReadBackMismatchError(expectedSha256, `sha256:<size:${st.size}>`);
    written = await readCappedBytes(abs, expectedSize);
  }
  const actual = sha256(written);
  if (actual !== expectedSha256) throw new ReadBackMismatchError(expectedSha256, actual);
}

export interface WriteDocxBytesOptions {
  /**
   * Preview-bound canonical cwd root for containment revalidation, instead
   * of the write call's cwd. The receipt owns the target and the
   * environment it was minted in; revalidating against the preview root
   * keeps a commit issued from a different call cwd from being misclassified
   * as a path race (and from becoming a retry loop).
   */
  root?: string;
  /**
   * Exact realpath the destination must still resolve to immediately before
   * rename (the receipt-bound canonical path). Rejects an in-window
   * symlink/alias swap that containment and source-hash checks alone cannot
   * catch: rename over a symlink would replace the link itself, silently
   * "succeeding" against the wrong namespace object.
   */
  expectedRealPath?: string;
  /**
   * Cap for the pre-write source rechecks (`assertExpectedSource`): the
   * host passes its `maxDocxBytes` so the source-check reads are stat-first
   * and capped. The preview-time source was at most this many bytes, so a
   * larger current file is a changed source, never an infrastructure error.
   */
  maxSourceBytes?: number;
}

/**
 * An infrastructure failure after rename(2): the atomic replacement landed.
 * The caller must classify this as a post-rename failure — the receipt is
 * consumed and the output must truthfully report the completed write — never
 * as a retryable pre-write failure.
 */
export class PostRenameError extends Error {
  readonly code = 'post_rename';
  constructor(readonly cause: unknown) {
    super(`failure after atomic rename: ${cause instanceof Error ? cause.message : String(cause)}`);
    this.name = 'PostRenameError';
  }
}

/** Atomic replacement: exclusive temp creation, fsync, rename, dir fsync, cleanup. */
export async function writeDocxBytes(
  cwd: string,
  abs: string,
  bytes: Uint8Array,
  expectedSource?: string,
  options?: WriteDocxBytesOptions,
): Promise<void> {
  const root = options?.root ?? (await realpath(resolve(cwd)));
  if (expectedSource !== undefined) await assertExpectedSource(abs, expectedSource, options?.maxSourceBytes);
  const parent = await revalidateParent(root, abs);
  await revalidateExisting(root, abs, parent);
  await mkdir(dirname(abs), { recursive: true });
  const tmp = join(dirname(abs), `.${basename(abs)}.${process.pid}.${randomBytes(10).toString('hex')}.tmp`);
  let fd;
  try {
    fd = await open(tmp, 'wx', 0o600);
    // Preserve the original file mode: the exclusive temp's 0o600 mode would
    // otherwise replace the target's mode on rename. Best effort: a vanished
    // or unreadable target simply keeps the 0o600 temp mode.
    const mode = await stat(abs)
      .then((st) => st.mode & 0o7777)
      .catch(() => undefined);
    if (mode !== undefined) await fd.chmod(mode);
    await revalidateExisting(root, abs, parent);
    await fd.writeFile(bytes);
    await fd.sync();
    await fd.close(); fd = undefined;
    if (expectedSource !== undefined) await assertExpectedSource(abs, expectedSource, options?.maxSourceBytes);
    if (options?.expectedRealPath !== undefined) {
      const actual = await realpath(abs);
      if (actual !== options.expectedRealPath) throw new PathChangedError(options.expectedRealPath, actual);
    }
    await revalidateExisting(root, abs, parent);
    await rename(tmp, abs);
    try {
      await fsyncDirIfSupported(dirname(abs));
    } catch (error) {
      // The replacement already landed: the caller must classify this as a
      // post-rename failure (receipt consumed, write reported as completed).
      throw new PostRenameError(error);
    }
  } finally {
    if (fd) await fd.close().catch(() => undefined);
    await unlink(tmp).catch(() => undefined);
  }
}

/** Create is intentionally exclusive: it can never overwrite an existing file. */
export async function createDocxBytes(cwd: string, abs: string, bytes: Uint8Array): Promise<void> {
  const root = await realpath(resolve(cwd));
  await mkdir(dirname(abs), { recursive: true });
  const parent = await revalidateParent(root, abs);
  await revalidateAbsent(root, abs, parent);
  const tmp = join(dirname(abs), `.${basename(abs)}.${process.pid}.${randomBytes(10).toString('hex')}.tmp`);
  try {
    const fd = await open(tmp, 'wx', 0o600);
    try { await revalidateAbsent(root, abs, parent); await fd.writeFile(bytes); await fd.sync(); } finally { await fd.close(); }
    // Hard-linking is atomic and fails with EEXIST, so creation cannot race
    // into an overwrite as rename would. Both paths are in the target dir.
    const { link } = await import('node:fs/promises');
    await revalidateAbsent(root, abs, parent);
    await link(tmp, abs);
  } finally { await unlink(tmp).catch(() => undefined); }
}

export class SourceChangedError extends Error {
  readonly code = 'source_changed';
  constructor(readonly expected: string, readonly actual: string) { super('source changed before commit'); }
}
