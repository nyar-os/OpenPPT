/**
 * Authoring types of `@wae/open-ppt`.
 *
 * This is the frozen P0 shape of the authoring surface: `new OpenPPT(options?)`
 * creates a session, `create`/`open` return an editable `Deck`, and every deck,
 * slide and element carries a stable ID plus a revision.
 *
 * Geometry in the authoring surface is points. The engine stores EMU internally
 * (`1pt = 12700 EMU`); screen pixels never enter the model.
 */

import type { DeckIntent, PresentationFormat } from './formats.js';

/** Opaque deck identity. */
export type DeckId = string;
/** Opaque slide identity; the index is order only. */
export type SlideId = string;
/** Opaque element identity. */
export type ElementId = string;
/** Opaque master identity. */
export type MasterId = string;
/** Opaque layout identity. */
export type LayoutId = string;
/** Opaque theme identity. */
export type ThemeId = string;
/** Opaque asset identity. */
export type AssetId = string;

/** One font file the engine may use for measurement and rendering. */
export interface FontSource {
    readonly family: string;
    readonly path?: string;
    readonly bytes?: Uint8Array;
}

/** Host-provided font lookup. Missing families must be reported, not swapped silently. */
export interface FontResolver {
    resolve(request: { readonly family: string; readonly weight?: number; readonly italic?: boolean }): FontSource | undefined;
}

/** Reference to external asset bytes. */
export interface AssetReference {
    readonly name: string;
    readonly mediaType?: string;
    readonly sha256?: string;
}

/** Host-provided asset lookup used when a deck references bytes it does not embed. */
export interface AssetResolver {
    resolve(reference: AssetReference): Uint8Array | Promise<Uint8Array>;
}

/** Hard limits applied before work starts. */
export interface ResourceBudgets {
    readonly maxPackageBytes?: number;
    readonly maxAssetBytes?: number;
    readonly maxSlides?: number;
}

/** Structured diagnostic emitted during import, inspection, render or export. */
export interface DeckDiagnostic {
    readonly code: string;
    readonly severity: 'info' | 'warning' | 'error';
    readonly revision: number;
    readonly slideId?: SlideId;
    readonly elementId?: ElementId;
    readonly recoverable: boolean;
    readonly message: string;
}

/** Session options. Constructing a session opens no window, file or socket. */
export interface OpenPPTOptions {
    /** BCP-47 tag used for diagnostics and default copy. Defaults to `en`. */
    readonly locale?: string;
    readonly fonts?: FontResolver | readonly FontSource[];
    readonly assets?: AssetResolver;
    readonly budgets?: ResourceBudgets;
    readonly diagnostics?: (event: DeckDiagnostic) => void;
}

/** Disclosure level. `partial` always ships a loss path and diagnostic codes. */
export type SupportLevel = 'supported' | 'partial' | 'unsupported';

/** Inspection rules understood by `deck.inspect()`. */
export type InspectCheck = 'overflow' | 'contrast' | 'alignment' | 'assets' | 'fonts' | 'out-of-bounds';

/** Per-page render targets. */
export type RenderFormat = 'svg' | 'png' | 'pdf';

/**
 * Formats `deck.export()` can write.
 *
 * `.pptx`, `.ppsx` and `.potx` share one OOXML package writer and differ only in
 * content type and show/template semantics; `.pdf` is a lossy static projection.
 */
export type ExportFormat = 'pptx' | 'ppsx' | 'potx' | 'pdf' | 'odp';

/** What the installed engine can do, per format and per capability. */
export interface FormatCapability {
    readonly import: SupportLevel;
    readonly edit: SupportLevel;
    readonly export: SupportLevel;
    readonly playback: SupportLevel;
    /** Codes reported when a `partial` path loses something. */
    readonly lossCodes: readonly string[];
}

/** Playback coverage, disclosed separately from static page rendering. */
export interface PlaybackCapability {
    readonly staticSlides: SupportLevel;
    readonly builds: SupportLevel;
    readonly transitions: SupportLevel;
    readonly media: SupportLevel;
}

/** Result of `OpenPPT.capabilities()`. Read this instead of assuming a method works. */
export interface OpenPPTCapabilities {
    readonly engine: { readonly package: string; readonly version: string };
    readonly formats: Readonly<Record<PresentationFormat, FormatCapability>>;
    readonly checks: Readonly<Record<InspectCheck, SupportLevel>>;
    readonly renderers: Readonly<Record<RenderFormat, SupportLevel>>;
    readonly exportTargets: Readonly<Record<ExportFormat, SupportLevel>>;
    /** Loss codes a partial export target may report, keyed like `exportTargets`. */
    readonly exportLossCodes?: Readonly<Partial<Record<ExportFormat, readonly string[]>>>;
    readonly playback: PlaybackCapability;
}

/** Explicit slide size in points. */
export interface CustomDeckSize {
    readonly widthPt: number;
    readonly heightPt: number;
}

/** Named ratios or an explicit point size. */
export type DeckSize = '16:9' | '4:3' | '16:10' | CustomDeckSize;

/** Minimal theme definition accepted by `create()` and `themes.apply()`. */
export interface ThemeDefinition {
    readonly name: string;
    readonly fonts?: Readonly<Record<string, string>>;
    readonly colors?: Readonly<Record<string, string>>;
}

/** Options of `OpenPPT.create()`. */
export interface DeckCreateOptions {
    readonly title?: string;
    readonly size?: DeckSize;
    readonly theme?: ThemeId | ThemeDefinition;
    /** One slide title per initial slide. */
    readonly slides?: readonly string[];
}

/**
 * Input of `OpenPPT.open()`.
 *
 * Exactly one of `path`, `bytes` or `project` must be present, so the engine
 * never guesses whether a string is a file name or serialized content.
 */
export type DeckInput =
    | { readonly path: string; readonly intent?: DeckIntent }
    | { readonly bytes: Uint8Array; readonly filename?: string; readonly intent?: DeckIntent }
    | { readonly project: string; readonly intent?: DeckIntent };

/** Point-based placement of an element on a slide. */
export interface Transform {
    readonly xPt: number;
    readonly yPt: number;
    readonly widthPt: number;
    readonly heightPt: number;
    readonly rotationDeg?: number;
}

/** One styled run inside a paragraph. */
export interface TextRun {
    readonly text: string;
    readonly bold?: boolean;
    readonly italic?: boolean;
    readonly underline?: boolean;
    readonly strike?: boolean;
    readonly sizePt?: number;
    readonly color?: string;
    readonly fontFamily?: string;
    readonly hyperlink?: string;
}

/** One paragraph. `runs` and `text` are mutually exclusive. */
export interface TextParagraph {
    readonly runs?: readonly TextRun[];
    readonly text?: string;
    readonly level?: number;
    readonly align?: 'left' | 'center' | 'right' | 'justify';
}

/** Rich text content of a text target. */
export interface RichText {
    readonly paragraphs: readonly TextParagraph[];
}

/** Accepted text input: plain string or structured paragraphs. */
export type TextInput = string | RichText;

/** How text that does not fit is handled. Never silently shrinks below readability. */
export interface TextSetOptions {
    readonly fit?: 'shrink' | 'wrap' | 'clip' | 'error';
    readonly expectedRevision?: number;
}

/** Readable and writable text slot, e.g. `slide.title` or `slide.notes`. */
export interface TextTarget {
    get(): Promise<string>;
    set(text: TextInput, options?: TextSetOptions): Promise<void>;
}

/** Element kinds the model distinguishes. */
export type ElementKind = 'text' | 'image' | 'shape' | 'group' | 'table' | 'chart' | 'formula' | 'media';

/** Read-only view of an element. Updates go through `ElementsApi.update()`. */
export interface Element {
    readonly id: ElementId;
    readonly kind: ElementKind;
    readonly transform: Transform;
    readonly alt: string | null;
    readonly locked: boolean;
}

/** Binding to a layout placeholder. */
export interface PlaceholderRef {
    readonly layoutId: LayoutId;
    readonly name: string;
}

/** Content sources accepted by media, image and asset inputs. */
export type BytesSource = { readonly path: string } | { readonly bytes: Uint8Array; readonly mediaType?: string };

/** Text element input. */
export interface TextElementInput {
    readonly text: TextInput;
    readonly transform?: Transform;
    readonly alt?: string;
    readonly placeholder?: PlaceholderRef;
}

/** Image element input. `alt` is required for accessibility. */
export interface ImageElementInput {
    readonly source: AssetId | BytesSource;
    readonly transform?: Transform;
    readonly alt: string;
    readonly placeholder?: PlaceholderRef;
}

/** Preset shape input. `geometry` is a preset name such as `rect` or `roundRect`. */
export interface ShapeElementInput {
    readonly geometry: string;
    readonly transform?: Transform;
    readonly fill?: string;
    readonly line?: { readonly color?: string; readonly widthPt?: number };
    readonly alt?: string;
}

/** Table cell input. */
export interface TableCell {
    readonly text?: TextInput;
    readonly fill?: string;
}

/** Table element input; rows must share the same column count. */
export interface TableElementInput {
    readonly rows: readonly (readonly TableCell[])[];
    readonly transform?: Transform;
    readonly alt?: string;
}

/** Chart families kept editable as data, not as a rendered bitmap. */
export type ChartType = 'column' | 'bar' | 'line' | 'area' | 'pie' | 'doughnut' | 'scatter' | 'radar';

/** One chart series. */
export interface ChartSeries {
    readonly name: string;
    readonly values: readonly number[];
}

/** Editable chart data. */
export interface ChartData {
    readonly categories: readonly string[];
    readonly series: readonly ChartSeries[];
}

/** Chart element input. */
export interface ChartElementInput {
    readonly type: ChartType;
    readonly data: ChartData;
    readonly alt: string;
    readonly transform?: Transform;
}

/** Formula element input. `expression` keeps the source, not only the picture. */
export interface FormulaElementInput {
    readonly expression: string;
    readonly alt?: string;
    readonly transform?: Transform;
}

/** Audio or video element input. */
export interface MediaElementInput {
    readonly kind: 'audio' | 'video';
    readonly source: AssetId | BytesSource;
    readonly alt: string;
    readonly transform?: Transform;
}

/** Run-level text style applied to every run of one text element. */
export interface TextStyle {
    readonly bold?: boolean;
    readonly italic?: boolean;
    readonly underline?: boolean;
    readonly strike?: boolean;
    /** Point size, 1–4000. */
    readonly sizePt?: number;
    /** Text colour as `#RGB` or `#RRGGBB`. */
    readonly color?: string;
    readonly fontFamily?: string;
    /** Paragraph alignment applied to every paragraph touched by the style. */
    readonly align?: 'left' | 'center' | 'right' | 'justify';
}

/** Partial element update addressed by ID. */
export interface ElementPatch {
    readonly transform?: Transform;
    readonly alt?: string;
    readonly text?: TextInput;
    readonly style?: TextStyle;
    /** Solid fill for a shape element; `null` clears the fill. */
    readonly fill?: string | null;
    /** Stroke for a shape element; `null` clears the line. */
    readonly line?: { readonly color?: string; readonly widthPt?: number } | null;
    readonly data?: ChartData;
    readonly expectedRevision?: number;
}

/** Editable flags of an element. */
export interface LockFlags {
    readonly move?: boolean;
    readonly resize?: boolean;
    readonly style?: boolean;
    readonly remove?: boolean;
}

/** Edge an element box is pinned to by `ElementsApi.align()`. */
export type AlignEdge = 'left' | 'center' | 'right' | 'top' | 'middle' | 'bottom';

/** Reference frame of an alignment run. Defaults to `slide`. */
export type AlignReference = 'slide' | 'selection';

/** Axis `ElementsApi.distribute()` spaces elements along. */
export type DistributeAxis = 'horizontal' | 'vertical';

/** Stacking commands for `ElementsApi.zOrder()`. */
export type ZOrder = 'front' | 'back' | 'forward' | 'backward';

/**
 * Edit operation that pins the selected boxes to one edge of the slide or of
 * their joint bounding box.
 *
 * `relativeTo` defaults to `slide`. Only `x`/`y` change, so sizes and rotation
 * stay untouched. Empty, duplicate or unknown element ids and unknown edges
 * answer with `session/argument-invalid`; a missing slide answers with
 * `object/not-found`.
 */
export interface ElementAlignOperation {
    readonly op: 'element.align';
    readonly slideId: SlideId;
    readonly elementIds: readonly ElementId[];
    readonly edge: AlignEdge;
    readonly relativeTo?: AlignReference;
}

/**
 * Edit operation that keeps the first and last box fixed and spaces the rest
 * with equal gaps between their outer boundaries.
 *
 * At least three element ids are required. Only the position on `axis` changes,
 * so sizes and the other axis stay untouched.
 */
export interface ElementDistributeOperation {
    readonly op: 'element.distribute';
    readonly slideId: SlideId;
    readonly elementIds: readonly ElementId[];
    readonly axis: DistributeAxis;
}

/** Placement and stacking operations addressed to elements of one slide. */
export type ElementOperation =
    | ElementReorderOperation
    | ElementDuplicateOperation
    | ElementAlignOperation
    | ElementDistributeOperation;

/**
 * Edit operation that rebuilds one element as a new element at the end of the
 * slide, optionally shifted by a point offset.
 *
 * Text, shapes, tables and images are supported; charts, formulas, media,
 * cropped images and placeholder-bound text answer with `object/unsupported`
 * because the public authoring inputs cannot express them without data loss.
 * An unknown slide or element answers with `object/not-found`.
 */
export interface ElementDuplicateOperation {
    readonly op: 'element.duplicate';
    readonly slideId: SlideId;
    readonly elementId: ElementId;
    /** Slide that receives the copy, default the source slide. */
    readonly targetSlideId?: SlideId;
    /** Horizontal shift in points, default 0. */
    readonly offsetXPt?: number;
    /** Vertical shift in points, default 0. */
    readonly offsetYPt?: number;
}

/**
 * Edit operation that moves one element to a zero-based paint index.
 *
 * Answers with `session/argument-invalid` when the index is outside the slide
 * and `object/not-found` when the slide or element does not exist.
 */
export interface ElementReorderOperation {
    readonly op: 'element.reorder';
    readonly slideId: SlideId;
    readonly elementId: ElementId;
    readonly index: number;
}

/**
 * Edit operation that paints a solid slide background, or clears it with `null`.
 *
 * `color` is a three- or six-digit hex colour and is normalised to `#RRGGBB`.
 */
export interface SlideSetBackgroundOperation {
    readonly op: 'slide.setBackground';
    readonly slideId: SlideId;
    readonly fill: { readonly kind: 'solid'; readonly color: string } | null;
}

/** Element operations of one slide. */
export interface ElementsApi {
    addText(input: TextElementInput): Promise<Element>;
    addImage(input: ImageElementInput): Promise<Element>;
    addShape(input: ShapeElementInput): Promise<Element>;
    addTable(input: TableElementInput): Promise<Element>;
    addChart(input: ChartElementInput): Promise<Element>;
    addFormula(input: FormulaElementInput): Promise<Element>;
    addMedia(input: MediaElementInput): Promise<Element>;
    get(elementId: ElementId): Promise<Element>;
    update(elementId: ElementId, patch: ElementPatch): Promise<Element>;
    remove(elementId: ElementId): Promise<void>;
    group(elementIds: readonly ElementId[], options?: { readonly alt?: string }): Promise<Element>;
    align(elementIds: readonly ElementId[], edge: AlignEdge, options?: { readonly relativeTo?: AlignReference }): Promise<void>;
    distribute(elementIds: readonly ElementId[], axis: DistributeAxis): Promise<void>;
    /** Copies one element onto the end of a slide, optionally shifted in points. */
    duplicate(
        elementId: ElementId,
        options?: { readonly toSlideId?: SlideId; readonly offsetX?: number; readonly offsetY?: number },
    ): Promise<Element>;
    zOrder(elementId: ElementId, order: ZOrder): Promise<void>;
    lock(elementId: ElementId, flags: LockFlags): Promise<void>;
}

/** Options of `slides.add()`. */
export interface SlideAddOptions {
    readonly layout?: LayoutId;
    readonly index?: number;
}

/** Compact slide listing for thumbnails and navigation. */
export interface SlideSummary {
    readonly id: SlideId;
    readonly index: number;
    readonly title: string;
    readonly hidden: boolean;
}

/** One slide. `index` is order only; `id` is identity. */
export interface Slide {
    readonly id: SlideId;
    readonly index: number;
    readonly title: TextTarget;
    readonly notes: TextTarget;
    readonly elements: ElementsApi;
    /** Hidden slides are skipped by playback but preserved on save. */
    hidden: boolean;
    layout: LayoutId | null;
}

/** Slide operations of a deck. */
export interface SlidesApi {
    add(options?: SlideAddOptions): Promise<Slide>;
    get(slideId: SlideId): Promise<Slide>;
    list(): Promise<readonly SlideSummary[]>;
    reorder(slideIds: readonly SlideId[]): Promise<void>;
    /**
     * Moves one slide to a zero-based position without re-listing the deck.
     *
     * Answers with `object/not-found` for an unknown slide and
     * `session/argument-invalid` when the index is outside the deck.
     */
    move(slideId: SlideId, index: number): Promise<void>;
    duplicate(slideId: SlideId, options?: { readonly index?: number }): Promise<Slide>;
    remove(slideId: SlideId): Promise<void>;
}

/** Master summary. */
export interface MasterInfo {
    readonly id: MasterId;
    readonly name: string;
    readonly layoutIds: readonly LayoutId[];
}

/** Layout summary, including its placeholders. */
export interface LayoutInfo {
    readonly id: LayoutId;
    readonly name: string;
    readonly masterId: MasterId | null;
    readonly placeholders: readonly string[];
}

/** Theme summary. */
export interface ThemeInfo {
    readonly id: ThemeId;
    readonly name: string;
}

/** Master operations. Editing master content lands after the first editing gate. */
export interface MastersApi {
    list(): Promise<readonly MasterInfo[]>;
}

/** Layout operations. */
export interface LayoutsApi {
    list(masterId?: MasterId): Promise<readonly LayoutInfo[]>;
}

/** Theme operations. `.thmx` packages arrive through `import()`, never through `open()`. */
export interface ThemesApi {
    list(): Promise<readonly ThemeInfo[]>;
    get(themeId: ThemeId): Promise<ThemeInfo>;
    apply(theme: ThemeId | ThemeDefinition): Promise<ThemeInfo>;
    import(input: { readonly path?: string; readonly bytes?: Uint8Array }): Promise<ThemeInfo>;
}

/** Asset summary. */
export interface AssetInfo {
    readonly id: AssetId;
    readonly name: string;
    readonly mediaType: string;
    readonly bytes: number;
    readonly sha256: string;
    readonly license?: string;
}

/** Asset input; bytes are embedded in the native project. */
export interface AssetInput {
    readonly name: string;
    readonly path?: string;
    readonly bytes?: Uint8Array;
    readonly mediaType?: string;
    readonly license?: string;
}

/** Asset operations. */
export interface AssetsApi {
    list(): Promise<readonly AssetInfo[]>;
    add(input: AssetInput): Promise<AssetInfo>;
    remove(assetId: AssetId): Promise<void>;
}

/**
 * Handle passed to `deck.transaction()`.
 *
 * Operations are buffered and committed as one native transaction against
 * `baseRevision`; a thrown error leaves no half page. Identities returned by
 * creation methods are provisional tokens valid only inside this transaction.
 */
export interface DeckTransaction {
    /** Revision the transaction started from. */
    readonly baseRevision: number;
    addSlide(options?: SlideAddOptions): SlideId;
    duplicateSlide(slideId: SlideId, options?: { readonly index?: number }): SlideId;
    removeSlide(slideId: SlideId): void;
    moveSlide(slideId: SlideId, index: number): void;
    reorderSlides(slideIds: readonly SlideId[]): void;
    setSlideTitle(slideId: SlideId, text: TextInput): void;
    setSlideNotes(slideId: SlideId, text: TextInput | null): void;
    setSlideHidden(slideId: SlideId, hidden: boolean): void;
    addText(slideId: SlideId, input: TextElementInput): ElementId;
    addImage(slideId: SlideId, input: ImageElementInput): ElementId;
    addShape(slideId: SlideId, input: ShapeElementInput): ElementId;
    addTable(slideId: SlideId, input: TableElementInput): ElementId;
    addChart(slideId: SlideId, input: ChartElementInput): ElementId;
    addFormula(slideId: SlideId, input: FormulaElementInput): ElementId;
    addMedia(slideId: SlideId, input: MediaElementInput): ElementId;
    updateElement(slideId: SlideId, elementId: ElementId, patch: ElementPatch): void;
    removeElement(slideId: SlideId, elementId: ElementId): void;
    addAsset(input: AssetInput): AssetId;
}

/** Options of `deck.inspect()`. */
export interface InspectOptions {
    readonly checks?: readonly InspectCheck[];
}

/** One finding of an inspection run. */
export interface InspectIssue {
    readonly code: string;
    readonly severity: 'info' | 'warning' | 'error';
    readonly revision: number;
    readonly slideId?: SlideId;
    readonly elementId?: ElementId;
    readonly recoverable: boolean;
    readonly message: string;
}

/** Result of `deck.inspect()`. Geometry and rules only, never an aesthetic score. */
export interface InspectReport {
    readonly revision: number;
    readonly checkerVersion: string;
    readonly issues: readonly InspectIssue[];
    readonly passed: boolean;
}

/** Options of `deck.render()`. */
export interface RenderOptions {
    readonly format: RenderFormat;
    /** Zero-based slide indexes to keep, or `'all'`. */
    readonly pages?: 'all' | readonly number[];
    readonly target?: string;
    readonly scale?: number;
}

/** One rendered page. */
export interface RenderPage {
    /** Zero-based slide index. */
    readonly index: number;
    readonly slideId: SlideId;
    readonly widthPx: number;
    readonly heightPx: number;
    /** Standalone SVG document; present for the `svg` renderer. */
    readonly svg: string;
    readonly path?: string;
    readonly bytes?: number;
}

/** One construct the renderer could not paint. */
export interface RenderLoss {
    readonly code: string;
    readonly slideId?: SlideId;
    readonly elementId?: ElementId;
    readonly message: string;
}

/** Result of `deck.render()`. */
export interface RenderReport {
    readonly revision: number;
    readonly format: RenderFormat;
    readonly pages: readonly RenderPage[];
    readonly losses: readonly RenderLoss[];
    readonly diagnostics: readonly DeckDiagnostic[];
}

/** Options of `deck.contactSheet()`. */
export interface ContactSheetOptions {
    /** SVG file to write the sheet to. */
    readonly target?: string;
    readonly format?: 'svg' | 'png';
    /** Thumbnail columns; defaults to 3. */
    readonly columns?: number;
    /** Gap between thumbnails in pixels; defaults to 12. */
    readonly gap?: number;
    /** Zero-based slide indexes to place, or `'all'`. Defaults to the whole deck. */
    readonly pages?: 'all' | readonly number[];
}

/** One thumbnail placed on a contact sheet, in sheet pixels. */
export interface ContactSheetTile {
    readonly index: number;
    readonly slideId: SlideId;
    readonly xPx: number;
    readonly yPx: number;
    readonly widthPx: number;
    readonly heightPx: number;
}

/** Whole-deck overview used for visual review. */
export interface ContactSheet {
    readonly revision: number;
    readonly slideIds: readonly SlideId[];
    readonly columns: number;
    readonly gapPx: number;
    readonly widthPx: number;
    readonly heightPx: number;
    readonly tiles: readonly ContactSheetTile[];
    /** Standalone SVG document nesting one `<svg>` per slide. */
    readonly svg: string;
    readonly bytes: number;
    readonly path?: string;
}

/** What kind of text a search hit came from: a slide title, notes or an element kind. */
export type SearchHitKind = ElementKind | 'title' | 'notes';

/** Options of `deck.search()`. */
export interface SearchOptions {
    /** Match exact letter case; defaults to `false`. */
    readonly caseSensitive?: boolean;
    /** Treat `query` as a regular expression instead of literal text; defaults to `false`. */
    readonly regex?: boolean;
    /** Also search speaker notes; defaults to `false`, which searches visible text only. */
    readonly notes?: boolean;
    /** Require a match to be delimited by non-word characters; defaults to `false`. */
    readonly wholeWord?: boolean;
    /** Maximum hits to return; defaults to 100. */
    readonly limit?: number;
    /** Search only these slide ids, in deck order; unknown ids are ignored. */
    readonly slideIds?: readonly SlideId[];
    /**
     * Report only hits of these kinds, such as `['title', 'text']`; unknown
     * kinds match nothing. Omitted means every kind.
     */
    readonly kinds?: readonly SearchHitKind[];
    /**
     * Report at most this many hits per slide. Hits beyond the cap are dropped
     * without changing `truncated`, because the cap is a filter, not a limit.
     */
    readonly maxPerSlide?: number;
}

/**
 * Options of `deck.replaceText()`.
 *
 * The matching options mirror {@link SearchOptions}; regular expressions,
 * `notes` and hit caps have no meaning for a rewrite and are not accepted.
 */
export interface ReplaceTextOptions {
    /** Match exact letter case; defaults to `false`. */
    readonly caseSensitive?: boolean;
    /** Only replace matches delimited by non-word characters; defaults to `false`. */
    readonly wholeWord?: boolean;
    /** Replace only on these slide ids, in deck order; unknown ids are ignored. */
    readonly slideIds?: readonly SlideId[];
    /** Reject the edit when the committed revision moved; same as `edit()`. */
    readonly expectedRevision?: number;
}

/**
 * One match of a search query.
 *
 * `text` is the complete text unit the match was found in (a slide title, a
 * text element, a table joined with tabs and newlines, or speaker notes when
 * `notes` was set), and `start`/`length` locate the match inside it. Title and
 * notes hits carry no `elementId`. `context` is a shorter excerpt around the
 * match, with `…` marking text that was cut off on either side.
 */
export interface SearchHit {
    readonly slideId: SlideId;
    readonly slideIndex: number;
    readonly elementId?: ElementId;
    readonly kind: SearchHitKind;
    readonly text: string;
    readonly start: number;
    readonly length: number;
    /** Up to 40 characters before and after the match, with `…` for clipped edges. */
    readonly context: string;
}

/**
 * Hits of `deck.search()`.
 *
 * This is a plain `SearchHit[]`; `truncated` is set when `limit` dropped
 * matches, so callers can ask for a narrower query or a larger limit.
 */
export type SearchHitList = SearchHit[] & { readonly truncated: boolean };

/** One slide of `deck.outline()`. */
export interface OutlineSlide {
    readonly id: SlideId;
    readonly index: number;
    readonly title: string;
    readonly notes?: string;
    readonly hidden: boolean;
    readonly elementCount: number;
    /** Body text lines, present when `outline({ points: true })` was asked for. */
    readonly points?: readonly string[];
}

/**
 * Whole-deck outline.
 *
 * The default projection reads the committed summary and never renders. Asking
 * for `points` also reads each slide's text elements, still without a render.
 */
export interface DeckOutline {
    readonly revision: number;
    readonly slides: readonly OutlineSlide[];
}

/** How lossy export treats findings. */
export type LossPolicy = 'error' | 'warn';

/** Options of `deck.export()`. */
export interface ExportOptions {
    readonly format: ExportFormat;
    readonly target: string;
    readonly lossPolicy?: LossPolicy;
    readonly overwrite?: boolean;
}

/** One thing a projection could not keep. */
export interface ExportLoss {
    readonly code: string;
    readonly severity: 'warning' | 'error';
    readonly slideId?: SlideId;
    readonly elementId?: ElementId;
    readonly message: string;
}

/**
 * One construct an editable import could not carry.
 *
 * Codes are stable (`import.pptx.*`) and location fields use the source package:
 * `slide` is one-based and `shape` is the authored shape id.
 */
export interface ImportLoss {
    readonly code: string;
    readonly status: 'partial' | 'unsupported';
    readonly slide: number | null;
    readonly shape: number | null;
    readonly message: string;
}

/** Result of `deck.export()`. Reports what the target actually kept. */
export interface ExportReport {
    readonly revision: number;
    readonly format: ExportFormat;
    readonly target: string;
    /** Size of the written package, in bytes. */
    readonly bytes: number;
    readonly losses: readonly ExportLoss[];
    readonly diagnostics: readonly DeckDiagnostic[];
    /** False for projections such as PDF that cannot be reopened for editing. */
    readonly editable: boolean;
}

/** Options of `deck.save()` (native project, not an exchange format). */
export interface SaveOptions {
    readonly target?: string;
    readonly overwrite?: boolean;
}

/** Result of `deck.save()`. */
export interface SaveReport {
    readonly target: string;
    readonly revision: number;
    readonly bytes: number;
}

/** Options of `deck.playback.create()`. */
export interface PlaybackOptions {
    readonly startAt?: SlideId;
    readonly fullscreen?: boolean;
}

/** Observable player state. */
export interface PlayerState {
    readonly revision: number;
    readonly slideId: SlideId;
    readonly buildStep: number;
    readonly timeMs: number;
    readonly playing: boolean;
}

/** Playback session. `next()` advances build steps before moving to the next slide. */
export interface Player {
    readonly state: PlayerState;
    start(): Promise<void>;
    pause(): Promise<void>;
    resume(): Promise<void>;
    next(): Promise<void>;
    previous(): Promise<void>;
    goTo(slideId: SlideId): Promise<void>;
    stop(): Promise<void>;
    dispose(): Promise<void>;
}

/** Playback entry point of a deck. */
export interface PlaybackApi {
    create(options?: PlaybackOptions): Promise<Player>;
}
