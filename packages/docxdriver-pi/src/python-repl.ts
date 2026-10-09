/**
 * Persistent Monty REPL runtime for the docxdriver extension.
 *
 * One crash-isolated Monty worker backs one `PythonRepl`: session state
 * (globals, classes, functions) survives across `execute()` calls. The
 * Python prelude and type stubs are constructor parameters supplied by the
 * V1 typed-plan extension (`python-plan-extension.ts`); the runtime itself
 * is surface-agnostic but has exactly one production caller.
 * User code is type-checked against the stubs before it runs; the prelude is
 * fed once at `create()` (skipping type check — it is trusted host code).
 * `executionNumber` counts every feed and later gates previews vs. commits.
 *
 * Cancellation is cooperative: Monty has no abort API for an in-flight
 * feed, so `execute` honors the Pi `AbortSignal` at checkpoints — at entry,
 * in the drive loop before/after host callbacks — and unwinds a suspended
 * feed with `resumeError` instead of abandoning it (an abandoned feed
 * poisons the session). An abort that fires while a call is queued behind
 * an in-flight feed resolves with the same `{ ok: false, error: 'aborted',
 * aborted: true }` result (never a bare rejection). Synchronous spans
 * (WASM, pure Python) are non-preemptible; the sandbox's own duration limit
 * is the backstop.
 */
import { CollectString, Monty, MontySession, FunctionSnapshot, NameLookupSnapshot, FutureSnapshot, MontyComplete } from '@pydantic/monty';
import {
  MontyCrashedError,
  MontyError,
  MontyRuntimeError,
  MontySyntaxError,
  MontyTypingError,
  ProtocolError,
  type CheckoutOptions,
  type Snapshot,
} from '@pydantic/monty';

/** Result of one `PythonRepl.execute` call. */
export interface PythonExecution {
  ok: boolean;
  stdout: string;
  value?: unknown;
  error?: string;
  reset: boolean;
  /** The Pi abort signal fired while this execution was in flight. */
  aborted?: boolean;
}

/** At most 1 MiB of `print()` output is collected per execution. */
const MAX_PRINT_BYTES = 1024 * 1024;

/**
 * A worker replacement or prelude reload failed after a crash: the runtime
 * is broken and must be discarded. The extension catches this, closes the
 * runtime best-effort, bumps the generation, invalidates every receipt, and
 * lets the next call create fresh state. A half-initialized runtime is never
 * retained.
 */
export class MontyUnrecoverableError extends Error {
  readonly code = 'monty_unrecoverable';
  constructor(message = 'python runtime failed and was discarded — session state lost; receipts expired') {
    super(message);
    this.name = 'MontyUnrecoverableError';
  }
}

/** Cooperative cancellation checkpoint: throws the stable `aborted` error. */
export class AbortedError extends Error {
  readonly code = 'aborted';
  constructor() {
    super('aborted');
    this.name = 'AbortedError';
  }
}

/** Cooperative cancellation checkpoint: throws the stable `aborted` error. */
export function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw new AbortedError();
}

/** Injectable seams for recovery-failure and print-cap regressions. */
export interface PythonReplOptions {
  /** Pool to check sessions out of; defaults to a fresh Monty pool. */
  pool?: Monty;
  /** Crash-recovery prelude reload; defaults to re-feeding the prelude. */
  reloadPrelude?: () => Promise<void>;
  /** `print()` collection cap in bytes; defaults to 1 MiB. */
  printCapBytes?: number;
}

/** Checkout configuration for a given type-stub set (varies per surface). */
function sessionOptions(stubs: string): CheckoutOptions {
  return {
    scriptName: 'docxdriver_repl.py',
    typeCheck: true,
    typeCheckStubs: stubs,
    typeCheckFormat: 'concise',
    limits: {
      maxDurationSecs: 10,
      maxMemory: 256 * 1024 * 1024,
      maxRecursionDepth: 500,
    },
  };
}

/**
 * A persistent, typed Python REPL over one Monty session.
 */
export class PythonRepl {
  private readonly pool: Monty;
  private readonly prelude: string;
  private readonly stubs: string;
  private readonly printCapBytes: number;
  private readonly injectedReload: (() => Promise<void>) | undefined;
  private session: MontySession;
  /** Serializes `execute()` calls so feeds never interleave on the session. */
  private tail: Promise<void> = Promise.resolve();
  private closed = false;
  /** Recovery failed (replacement or prelude reload): the runtime is broken. */
  private broken = false;
  /** Monotonic feed counter; a preview's number gates later commits. */
  private counter = 0;

  private constructor(
    pool: Monty,
    session: MontySession,
    prelude: string,
    stubs: string,
    printCapBytes: number,
    injectedReload: (() => Promise<void>) | undefined,
  ) {
    this.pool = pool;
    this.session = session;
    this.prelude = prelude;
    this.stubs = stubs;
    this.printCapBytes = printCapBytes;
    this.injectedReload = injectedReload;
  }

  /**
   * Creates the pool, checks out one session, and feeds the prelude once.
   * The returned runtime keeps its Python state until `close()`.
   */
  static async create(prelude: string, stubs: string, options?: PythonReplOptions): Promise<PythonRepl> {
    const pool = options?.pool ?? (await Monty.create({ minProcesses: 1, maxProcesses: 1, requestTimeout: 15 }));
    const ownsPool = options?.pool === undefined;
    try {
      const session = await pool.checkout(sessionOptions(stubs));
      const repl = new PythonRepl(pool, session, prelude, stubs, options?.printCapBytes ?? MAX_PRINT_BYTES, options?.reloadPrelude);
      await repl.feedPrelude();
      return repl;
    } catch (error) {
      if (ownsPool) await pool.close().catch(() => undefined);
      throw error;
    }
  }

  /**
   * OS pid of the backing Monty worker, or `undefined` while a feed is in
   * flight. Mirrors `MontySession.workerPid`, which monty documents as a
   * diagnostics/tests surface; the crash-recovery regression test uses it to
   * SIGKILL the worker.
   */
  get workerPid(): number | undefined {
    return this.session.workerPid;
  }

  /** Monotonic feed counter; a preview's number gates later commits. */
  get executionNumber(): number {
    return this.counter;
  }

  /**
   * Runs one snippet. Errors surface as `{ ok: false, error }` rather than
   * rejections, except after `close()` or an unrecoverable recovery failure
   * (`MontyUnrecoverableError`). A crashed worker invalidates the session:
   * the runtime replaces it and reports `reset: true` (the user's code is
   * never silently retried). `signal` is the Pi abort signal: an aborted
   * call is answered immediately without consuming an execution number or
   * starting any host work; a mid-feed abort unwinds the feed cooperatively
   * and is reported as `{ ok: false, error: 'aborted', aborted: true }`.
   */
  execute(code: string, externalLookup: Record<string, unknown>, signal?: AbortSignal): Promise<PythonExecution> {
    if (signal?.aborted) {
      // Aborted before the feed was even queued: no execution number is
      // consumed and no host work happens.
      return Promise.resolve({ ok: false, stdout: '', error: 'aborted', reset: false, aborted: true });
    }
    const run = this.tail
      .then(() => this.executeNow(code, externalLookup, signal))
      .catch((error) => {
        if (error instanceof AbortedError && signal?.aborted) {
          // The abort fired while this call was queued behind an in-flight
          // feed: answer with the normal aborted result (never a bare
          // rejection), matching the documented cancellation contract — no
          // execution number is consumed and no host work happens.
          return { ok: false, stdout: '', error: 'aborted', reset: false, aborted: true };
        }
        throw error;
      });
    this.tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  /** Closes the session, then the pool. Idempotent: later calls no-op. */
  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    await this.tail;
    try {
      await this.session.close();
    } finally {
      await this.pool.close();
    }
  }

  /** The initial prelude feed at create time: always the real prelude. */
  private async feedPrelude(): Promise<void> {
    await this.session.feedRun(this.prelude, { skipTypeCheck: true });
  }

  /** Crash-recovery prelude reload (injectable for recovery-failure tests). */
  private async reloadPrelude(): Promise<void> {
    if (this.injectedReload !== undefined) {
      await this.injectedReload();
      return;
    }
    await this.session.feedRun(this.prelude, { skipTypeCheck: true });
  }

  private async executeNow(
    code: string,
    externalLookup: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<PythonExecution> {
    if (this.closed) {
      throw new Error('PythonRepl is closed');
    }
    if (this.broken) {
      throw new MontyUnrecoverableError();
    }
    throwIfAborted(signal);
    this.counter += 1;
    const collect = new CollectString(this.printCapBytes);
    const aborted = (): boolean => signal?.aborted === true;
    try {
      const value = await this.drive(code, externalLookup, collect, signal);
      return { ok: true, stdout: collect.output, value, reset: false, aborted: aborted() ? true : undefined };
    } catch (error) {
      if (error instanceof MontyCrashedError || error instanceof ProtocolError) {
        // May throw MontyUnrecoverableError when replacement or prelude
        // reload fails: the runtime is broken and must be discarded.
        return this.recoverAfterCrash(error);
      }
      return {
        ok: false,
        stdout: collect.output,
        error: formatMontyError(error),
        reset: false,
        aborted: aborted() ? true : undefined,
      };
    }
  }

  /**
   * Drives one snippet with `feedStart` instead of `feedRun`, so promise-
   * returning external callbacks are awaited in JS *before* the sandbox is
   * resumed with their value. `feedRun` would register them as Monty
   * futures: the wrapper discards the future object, the host side effects
   * (file writes, notices) would race the feed, and host rejections would be
   * swallowed. Awaiting here keeps the sync Python prelude correct and
   * race-free: Python sees the resolved value, or the exception.
   *
   * Abort checkpoints: at the top of the snapshot loop (before any host
   * callback is invoked) and after each host callback resolves (before its
   * value is resumed). Either checkpoint unwinds the feed with
   * `resumeError(new Error('aborted'))`; the feed always runs to completion
   * — a suspended feed is never abandoned. NameLookup/Future snapshots
   * cannot take a `resumeError`, so they are answered normally and the loop
   * reaches the next function checkpoint (or completion).
   */
  private async drive(
    code: string,
    externalLookup: Record<string, unknown>,
    collect: CollectString,
    signal?: AbortSignal,
  ): Promise<unknown> {
    let snapshot: Snapshot = await this.session.feedStart(code, {
      externalLookup,
      printCallback: collect,
    });
    const abortError = new AbortedError();
    for (;;) {
      if (snapshot instanceof MontyComplete) {
        return snapshot.output;
      }
      if (signal?.aborted) {
        if (snapshot instanceof FunctionSnapshot) {
          // Cooperative unwind: the sandbox sees RuntimeError('aborted');
          // Python catching it cannot re-enter host work (every `_docx_*`
          // callback begins with its own abort check).
          snapshot = await snapshot.resumeError(abortError);
          continue;
        }
        // Not a function snapshot: keep answering normally so the feed
        // progresses to the next function checkpoint or completion.
      }
      if (snapshot instanceof FunctionSnapshot) {
        if (snapshot.isOsFunction) {
          // No `os` callback: decline so the sandbox raises the default
          // exception (filesystem/network access stays denied).
          snapshot = await snapshot.resumeNotHandled();
          continue;
        }
        if (snapshot.isMethodCall) {
          // Mirrors monty's automatic dispatch: no host-side class registry.
          snapshot = await snapshot.resumeError(
            new Error(`method calls on host objects are not supported: ${snapshot.functionName}`),
          );
          continue;
        }
        if (!Object.prototype.hasOwnProperty.call(externalLookup, snapshot.functionName)) {
          snapshot = await snapshot.resumeNotFound();
          continue;
        }
        const entry = externalLookup[snapshot.functionName];
        if (typeof entry !== 'function') {
          snapshot = await snapshot.resumeNotFound();
          continue;
        }
        // Positional args plus kwargs as Monty's trailing-object convention.
        const args = [...snapshot.args];
        if (Object.keys(snapshot.kwargs).length > 0) {
          args.push(snapshot.kwargs);
        }
        let value: unknown;
        try {
          value = await (entry as (...args: unknown[]) => unknown)(...args);
        } catch (error) {
          // A host throw crosses into the sandbox as a Python exception
          // (RuntimeError unless the error name matches a Python type).
          snapshot = await snapshot.resumeError(error);
          continue;
        }
        if (signal?.aborted) {
          // The callback settled, but cancellation arrived while it ran:
          // discard its value and unwind the feed. Its side effects (e.g. a
          // stored receipt) are already visible through the control output.
          snapshot = await snapshot.resumeError(abortError);
          continue;
        }
        snapshot = await snapshot.resume(value);
        continue;
      }
      if (snapshot instanceof NameLookupSnapshot) {
        // Resolves the name from the captured externalLookup: functions by
        // name (the following turn is a FunctionSnapshot this loop awaits),
        // plain values directly, absent names as NameError.
        snapshot = await snapshot.resumeAuto();
        continue;
      }
      if (snapshot instanceof FutureSnapshot) {
        // Should not arise: every callback is awaited before resume, so no
        // external future is ever registered. If one does, resumeAuto finds
        // no tracked promise and poisons the session via ProtocolError —
        // the crash-recovery path below reports it as a reset.
        snapshot = await snapshot.resumeAuto();
        continue;
      }
      // Defensive: the Snapshot union above is exhaustive; a new monty
      // snapshot kind would otherwise spin forever instead of failing.
      throw new Error('unexpected monty snapshot kind');
    }
  }

  /**
   * The worker died or the protocol broke: the old session is worthless.
   * Discard it, check out a fresh session, reload the prelude, and report the
   * failure with `reset: true` so callers know state was lost.
   *
   * If the replacement or the prelude reload fails, the runtime is broken:
   * the half-initialized session is closed, `MontyUnrecoverableError` is
   * thrown, and the caller must discard the runtime (the extension bumps the
   * generation and invalidates receipts so the next call creates fresh
   * state). A half-initialized runtime is never retained.
   */
  private async recoverAfterCrash(error: unknown): Promise<PythonExecution> {
    await this.session.close().catch(() => undefined);
    try {
      this.session = await this.pool.checkout(sessionOptions(this.stubs));
      await this.reloadPrelude();
    } catch (recoveryError) {
      await this.session.close().catch(() => undefined);
      this.broken = true;
      throw new MontyUnrecoverableError();
    }
    return { ok: false, stdout: '', error: formatMontyError(error), reset: true };
  }
}

function formatMontyError(error: unknown): string {
  if (error instanceof MontyRuntimeError || error instanceof MontySyntaxError) {
    return error.display('traceback');
  }
  if (error instanceof MontyTypingError) {
    return error.display();
  }
  if (error instanceof MontyError) {
    return error.display('type-msg');
  }
  return error instanceof Error ? error.message : String(error);
}
