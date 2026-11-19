/**
 * Native addon discovery.
 *
 * The addon only maps values: deck semantics, saving and playback stay in the
 * shared engine. An absent addon is a declarative state, so `capabilities()`
 * reports `capability/not-implemented` instead of inventing a matrix.
 */

import { createRequire } from 'node:module';

const requireNative = createRequire(import.meta.url);

/** Functions exported by the built addon. */
export interface NativeBinding {
    version(): string;
    capabilities(): string;
    create(...args: unknown[]): unknown;
    open(...args: unknown[]): unknown;
    inspect(...args: unknown[]): unknown;
}

let cached: NativeBinding | null | undefined;

/** Loads the addon once, or returns `undefined` when it is not installed. */
export function nativeBinding(): NativeBinding | undefined {
    if (cached === undefined) {
        cached = load();
    }
    return cached ?? undefined;
}

function load(): NativeBinding | null {
    for (const candidate of candidates()) {
        try {
            return requireNative(candidate) as NativeBinding;
        } catch (error) {
            if (candidate === explicitPath()) {
                throw error;
            }
            if (!isModuleNotFound(error, candidate)) {
                throw error;
            }
        }
    }
    return null;
}

function candidates(): string[] {
    const explicit = explicitPath();
    if (explicit) {
        return [explicit];
    }
    return [`@wae/open-ppt-${process.platform}-${process.arch}`];
}

function explicitPath(): string | undefined {
    const value = process.env.OPEN_PPT_NATIVE;
    return value !== undefined && value.length > 0 ? value : undefined;
}

function isModuleNotFound(error: unknown, candidate: string): boolean {
    if (typeof error !== 'object' || error === null) {
        return false;
    }
    const code = (error as { code?: unknown }).code;
    return code === 'MODULE_NOT_FOUND' && (error as Error).message.includes(candidate);
}
