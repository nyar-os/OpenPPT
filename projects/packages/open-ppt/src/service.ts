/**
 * Orthogonal native operations.
 *
 * One function performs one operation and returns data; argument parsing, help
 * text and exit codes belong to `cli.ts` and to the authoring surface. Nothing
 * here knows about a command line.
 */

import { NotImplementedError, OpenPPTError, errorCodes, type OpenPPTErrorCode } from './errors.js';
import { nativeBinding } from './native.js';
import type { InspectCheck, InspectReport, OpenPPTCapabilities } from './types.js';

/** One slide of a deck summary. */
export interface DeckSummarySlide {
    readonly index: number;
    readonly id: string;
    readonly title: string;
    readonly hidden: boolean;
    readonly elements: number;
    readonly notes: string | null;
}

/** Summary of one deck at its committed revision. */
export interface DeckSummary {
    readonly id: string;
    readonly revision: number;
    readonly title: string | null;
    readonly size: { readonly widthPt: number; readonly heightPt: number };
    readonly layouts: ReadonlyArray<{ readonly id: string; readonly name: string }>;
    readonly slides: ReadonlyArray<DeckSummarySlide>;
}

/** Result of creating or opening a deck. */
export interface DeckHandle {
    readonly handle: number;
    readonly deck: DeckSummary;
}

/** Result of writing a native project. */
export interface SaveReport {
    readonly path: string;
    readonly revision: number;
    readonly slides: number;
    readonly bytes: number;
}

/** One static playback frame. */
export interface PlayFrame {
    readonly revision: number;
    readonly slideId: string | null;
    readonly buildStep: number;
    readonly timeMs: number;
    readonly notes: string | null;
}

/** Result of walking the static show. */
export interface PlayReport {
    readonly frames: ReadonlyArray<PlayFrame>;
}

/** Arguments of {@link create}. */
export interface CreateDeckRequest {
    readonly title?: string;
    readonly size?: string | { readonly widthPt: number; readonly heightPt: number };
    readonly theme?: string;
    readonly slides?: ReadonlyArray<string>;
}

/** One typed operation of an edit script. */
export type EditOperation = Record<string, unknown>;

/** Atomic edit request; `expectedRevision` rejects a stale writer. */
export interface EditRequest {
    readonly expectedRevision?: number;
    readonly operations: ReadonlyArray<EditOperation>;
}

/** One element of a slide detail. */
export interface ElementDetail {
    readonly id: string;
    readonly kind: string;
    readonly transform: {
        readonly xPt: number;
        readonly yPt: number;
        readonly widthPt: number;
        readonly heightPt: number;
        readonly rotationDeg: number | null;
    };
    readonly alt: string | null;
    readonly locked: boolean;
    readonly text?: string;
}

/** One slide with its element tree. */
export interface SlideDetail {
    readonly id: string;
    readonly index: number;
    readonly title: string;
    readonly hidden: boolean;
    readonly layout: string | null;
    readonly notes: string | null;
    readonly elements: ReadonlyArray<ElementDetail>;
}

/** Result of one committed edit script. */
export interface EditReport {
    readonly deck: DeckSummary;
    readonly revision: number;
    readonly results: ReadonlyArray<string | null>;
    readonly resolved: Readonly<Record<string, string>>;
}

/** Result of one undo or redo. */
export interface HistoryReport {
    readonly deck: DeckSummary;
    readonly revision: number;
    readonly changed: boolean;
}

/** One embedded asset without its bytes. */
export interface AssetRecord {
    readonly id: string;
    readonly name: string;
    readonly mediaType: string;
    readonly bytes: number;
    readonly sha256: string;
    readonly license: string | null;
}

const knownCodes = new Set<string>(errorCodes);

function binding(feature: string) {
    const native = nativeBinding();
    if (!native) {
        throw new NotImplementedError(feature);
    }
    return native;
}

function decode<T>(work: () => string): T {
    try {
        return JSON.parse(work()) as T;
    } catch (error) {
        throw translate(error);
    }
}

function translate(error: unknown): OpenPPTError {
    if (error instanceof OpenPPTError) {
        return error;
    }
    const message = error instanceof Error ? error.message : String(error);
    const separator = message.indexOf(': ');
    const code = separator > 0 ? message.slice(0, separator) : '';
    if (knownCodes.has(code)) {
        return new OpenPPTError(code as OpenPPTErrorCode, message.slice(separator + 2), { details: { feature: code } });
    }
    return new OpenPPTError('format/invalid-package', message, { cause: error });
}

/** Reads the capability matrix of the installed addon. */
export function capabilities(): OpenPPTCapabilities {
    return decode<OpenPPTCapabilities>(() => binding('capabilities').capabilities());
}

/** Creates a deck and returns its handle. */
export function create(request: CreateDeckRequest): DeckHandle {
    return decode<DeckHandle>(() =>
        binding('create').deck_create(
            JSON.stringify({ title: request.title, size: request.size, theme: request.theme, slides: request.slides ?? [] }),
        ),
    );
}

/** Opens a native project and returns its handle. */
export function open(path: string): DeckHandle {
    return decode<DeckHandle>(() => binding('open').deck_open(path));
}

/** Reads the summary of an open deck. */
export function summary(handle: number): DeckSummary {
    return decode<DeckSummary>(() => binding('deck.summary').deck_summary(handle));
}

/** Writes one open deck to a native project path. */
export function save(handle: number, target: string, overwrite: boolean): SaveReport {
    return decode<SaveReport>(() => binding('deck.save').deck_save(handle, target, overwrite));
}

/** Runs the rule checks of one open deck. */
export function inspect(handle: number, checks?: ReadonlyArray<InspectCheck>): InspectReport {
    return decode<InspectReport>(() =>
        checks === undefined ? binding('deck.inspect').deck_inspect(handle) : binding('deck.inspect').deck_inspect(handle, JSON.stringify(checks)),
    );
}

/** Walks the static show of one open deck. */
export function play(handle: number): PlayReport {
    return decode<PlayReport>(() => binding('deck.play').deck_play(handle));
}

/** Applies one atomic edit script. */
export function edit(handle: number, request: EditRequest): EditReport {
    return decode<EditReport>(() => binding('deck.edit').deck_edit(handle, JSON.stringify(request)));
}

/** Reads one slide with its elements. */
export function slide(handle: number, slideId: string): SlideDetail {
    return decode<SlideDetail>(() => binding('deck.slide').deck_slide(handle, slideId));
}

/** Lists embedded assets without bytes. */
export function assets(handle: number): ReadonlyArray<AssetRecord> {
    return decode<{ readonly assets: ReadonlyArray<AssetRecord> }>(() => binding('deck.assets').deck_assets(handle)).assets;
}

/** Restores the previous committed document. */
export function undo(handle: number): HistoryReport {
    return decode<HistoryReport>(() => binding('deck.undo').deck_undo(handle));
}

/** Reapplies the last undone document. */
export function redo(handle: number): HistoryReport {
    return decode<HistoryReport>(() => binding('deck.redo').deck_redo(handle));
}

/** Releases one deck handle. */
export function close(handle: number): void {
    binding('deck.close').deck_close(handle);
}
