/**
 * Native addon discovery.
 *
 * The addon only maps values: deck semantics, saving and playback stay in the
 * shared engine. An absent addon is a declarative state, so `capabilities()`
 * reports `capability/not-implemented` instead of inventing a matrix.
 */

import { createRequire } from 'node:module';

const requireNative = createRequire(import.meta.url);

/** Orthogonal exports of the built addon. Every call returns a JSON string. */
export interface NativeBinding {
    version(): string;
    capabilities(): string;
    deck_create(requestJson: string): string;
    deck_open(path: string): string;
    deck_summary(handle: number): string;
    deck_save(handle: number, path: string, overwrite: boolean): string;
    deck_inspect(handle: number, checksJson?: string): string;
    deck_play(handle: number): string;
    deck_edit(handle: number, requestJson: string): string;
    deck_slide(handle: number, slideId: string): string;
    deck_assets(handle: number): string;
    deck_asset(handle: number, assetId: string): string;
    deck_undo(handle: number): string;
    deck_redo(handle: number): string;
    deck_close(handle: number): boolean;
    openGui(options?: { title?: string; contentDir?: string; width?: number; height?: number }): string;
    pumpEvents(): string;
    closeGui(): string;
    isGuiOpen(): boolean;
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
