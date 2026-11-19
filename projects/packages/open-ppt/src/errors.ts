/**
 * Stable error surface of `@wae/open-ppt`.
 *
 * Codes are contract, not prose: callers compare them, localize around them and
 * never parse the message. A failure therefore keeps its code even when the
 * text is translated.
 */

/** Every stable code the authoring surface may report. */
export const errorCodes = [
    'session/disposed',
    'session/argument-invalid',
    'session/budget-exceeded',
    'deck/not-found',
    'deck/revision-conflict',
    'object/not-found',
    'object/type-mismatch',
    'object/unsupported',
    'format/unsupported',
    'format/invalid-package',
    'format/encrypted',
    'format/macro-refused',
    'asset/missing',
    'asset/too-large',
    'font/missing',
    'render/backend-unavailable',
    'export/loss-exceeded',
    'playback/invalid-state',
    'capability/not-implemented',
] as const;

/** Union of {@link errorCodes}. */
export type OpenPPTErrorCode = (typeof errorCodes)[number];

/** Optional locators a failure can carry so callers can point at the object. */
export interface OpenPPTErrorDetails {
    readonly revision?: number;
    readonly slideId?: string;
    readonly elementId?: string;
    /** Presentation format involved in the failure, without the leading dot. */
    readonly format?: string;
    /** Authoring capability involved in the failure, e.g. `deck.export`. */
    readonly feature?: string;
}

/** Options accepted by {@link OpenPPTError}. */
export interface OpenPPTErrorOptions {
    readonly recoverable?: boolean;
    readonly details?: OpenPPTErrorDetails;
    readonly cause?: unknown;
}

/** Every failure thrown or rejected by this package. */
export class OpenPPTError extends Error {
    /** Stable, comparable code. */
    readonly code: OpenPPTErrorCode;
    /** Whether retrying with different input or state can succeed. */
    readonly recoverable: boolean;
    /** Optional locators (`revision`, `slideId`, `elementId`, `format`, `feature`). */
    readonly details: OpenPPTErrorDetails;

    constructor(code: OpenPPTErrorCode, message: string, options: OpenPPTErrorOptions = {}) {
        super(message, options.cause === undefined ? undefined : { cause: options.cause });
        this.name = 'OpenPPTError';
        this.code = code;
        this.recoverable = options.recoverable ?? false;
        this.details = Object.freeze({ ...options.details });
    }
}

/**
 * Reported by declared methods whose backing capability is not wired yet.
 *
 * The scaffold uses one code for every unimplemented entry point instead of
 * inventing per-method placeholders; each method drops this rejection as it is
 * implemented, and the code disappears with the last one.
 */
export class NotImplementedError extends OpenPPTError {
    constructor(feature: string, message = `\`${feature}\` is not implemented yet`) {
        super('capability/not-implemented', message, { recoverable: false, details: { feature } });
        this.name = 'NotImplementedError';
    }
}

/** Narrows an unknown thrown value to {@link OpenPPTError}. */
export function isOpenPPTError(value: unknown): value is OpenPPTError {
    return value instanceof OpenPPTError;
}
