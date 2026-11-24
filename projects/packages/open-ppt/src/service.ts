/**
 * Orthogonal native operations.
 *
 * One function performs one operation and returns data; argument parsing, help
 * text and exit codes belong to `cli.ts` and to the authoring surface. Nothing
 * here knows about a command line.
 */

import { NotImplementedError, OpenPPTError, errorCodes, renderErrorCopy, type OpenPPTErrorCode, type OpenPPTErrorParams } from './errors.js';
import { nativeBinding } from './native.js';
import type {
    ExportFormat,
    ExportReport,
    ImportLoss,
    InspectCheck,
    InspectReport,
    LossPolicy,
    OpenPPTCapabilities,
    RenderReport,
} from './types.js';

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
    /** Renderable payload of the element; tables expose their rows and cell text here. */
    readonly content?: {
        readonly rows?: ReadonlyArray<{
            readonly cells?: ReadonlyArray<{ readonly text?: string }>;
        }>;
    } | null;
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
    const native = error instanceof Error ? (error as Error & { code?: unknown; detail?: unknown }) : undefined;
    const message = native ? native.message : String(error);
    const rawCode = native?.code;
    const code = typeof rawCode === 'string' ? rawCode : undefined;
    if (code !== undefined) {
        const params = readParams(native?.detail);
        return new OpenPPTError(code as OpenPPTErrorCode, renderErrorCopy(code, params, message), { params, cause: error });
    }
    // Compatibility entry for addons older than the structured contract, which
    // concatenated `code: message` into the Error text. Current bindings always
    // carry `code` and `detail.params`, so this split never runs against them.
    const separator = message.indexOf(': ');
    const legacy = separator > 0 ? message.slice(0, separator) : '';
    if (knownCodes.has(legacy)) {
        return new OpenPPTError(legacy as OpenPPTErrorCode, message.slice(separator + 2), { cause: error });
    }
    return new OpenPPTError('binding/failure', message, { cause: error });
}

function readParams(detail: unknown): OpenPPTErrorParams {
    if (typeof detail === 'object' && detail !== null) {
        const params = (detail as { params?: unknown }).params;
        if (typeof params === 'object' && params !== null && !Array.isArray(params)) {
            return params as OpenPPTErrorParams;
        }
    }
    return {};
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

/** Result of importing a PresentationML package as an editable deck. */
export interface ImportedDeckHandle extends DeckHandle {
    readonly losses: ReadonlyArray<ImportLoss>;
}

/** Imports a `.pptx`-family package into an editable deck handle. */
export function importPptx(path: string): ImportedDeckHandle {
    return decode<ImportedDeckHandle>(() => binding('import').deck_import_pptx(path));
}

/** Reads the summary of an open deck. */
export function summary(handle: number): DeckSummary {
    return decode<DeckSummary>(() => binding('deck.summary').deck_summary(handle));
}

/** One slide of the projected outline, including talking points. */
export interface OutlineSlideReport {
    readonly id: string;
    readonly index: number;
    readonly title: string;
    readonly hidden: boolean;
    readonly elements: number;
    readonly notes: string | null;
    readonly points: ReadonlyArray<string>;
}

/** Result of the projected outline. */
export interface OutlineReport {
    readonly revision: number;
    readonly slides: ReadonlyArray<OutlineSlideReport>;
}

/** Projects one entry per slide with titles, notes and body lines. */
export function outline(handle: number, request: { readonly maxPointsPerSlide?: number } = {}): OutlineReport {
    return decode<OutlineReport>(() => binding('deck.outline').deck_outline(handle, JSON.stringify(request)));
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

/** Renders every slide of one open deck to standalone SVG pages. */
export function render(handle: number, request: { readonly format?: 'svg' } = {}): RenderReport {
    return decode<RenderReport>(() => binding('deck.render').deck_render(handle, JSON.stringify(request)));
}

/** Arguments of {@link exportDeck}. */
export interface ExportDeckRequest {
    readonly format: ExportFormat;
    readonly target: string;
    readonly overwrite?: boolean;
    readonly lossPolicy?: LossPolicy;
}

/** Writes one open deck as a PresentationML package. */
export function exportDeck(handle: number, request: ExportDeckRequest): ExportReport {
    return decode<ExportReport>(() => binding('deck.export').deck_export(handle, JSON.stringify(request)));
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

/** One painted slide of an imported presentation. */
export interface PptxSlideFrame {
    readonly index: number;
    readonly widthPx: number;
    readonly heightPx: number;
    /** Standalone SVG document painted at 96 dpi. */
    readonly svg: string;
}

/** One import or view issue of an imported presentation. */
export interface PptxIssue {
    readonly code: string;
    readonly status: 'partial' | 'unsupported';
    readonly message: string;
    readonly slide: number | null;
    readonly shape: number | null;
}

/** Show summary of one imported presentation. */
export interface PptxPresentation {
    readonly label: string;
    readonly title: string | null;
    readonly size: {
        readonly widthPx: number;
        readonly heightPx: number;
        readonly widthPt: number;
        readonly heightPt: number;
    };
    /** Visible slides in show order; hidden slides are already skipped. */
    readonly slides: ReadonlyArray<{ readonly index: number; readonly widthPx: number; readonly heightPx: number }>;
    readonly issues: ReadonlyArray<PptxIssue>;
}

/** Result of opening a PresentationML package for playback. */
export interface PptxHandle {
    readonly handle: number;
    readonly presentation: PptxPresentation;
}

/** Imports a PresentationML package and paints every visible slide. */
export function openPptx(path: string): PptxHandle {
    return decode<PptxHandle>(() => binding('pptx.open').pptx_open(path));
}

/** Reads one painted slide of an imported presentation by its one-based number. */
export function pptxSlide(handle: number, index: number): PptxSlideFrame {
    return decode<{ readonly slide: PptxSlideFrame }>(() => binding('pptx.slide').pptx_slide(handle, index)).slide;
}

/** Releases one playback handle. */
export function closePptx(handle: number): void {
    binding('pptx.close').pptx_close(handle);
}
