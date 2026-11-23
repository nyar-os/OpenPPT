/**
 * Editable deck handle.
 *
 * One instance wraps one native handle. Decks come from `OpenPPT.create()` or
 * `OpenPPT.open()`; the constructor is private so no second deck model can be
 * built here. Every mutation is one atomic native transaction, so a rejected
 * call leaves no half page behind.
 */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { basename, dirname, extname, join } from 'node:path';

import { buildContactSheetSvg } from './contact-sheet.js';
import { NotImplementedError, OpenPPTError } from './errors.js';
import type { DeckIntent, PresentationFormat } from './formats.js';
import { buildOutline } from './outline.js';
import { searchSlides } from './search.js';
import * as service from './service.js';
import type { DeckHandle, DeckSummary } from './service.js';
import type {
    AlignEdge,
    AlignReference,
    AssetId,
    AssetInfo,
    AssetInput,
    AssetsApi,
    BytesSource,
    ChartElementInput,
    ContactSheet,
    ContactSheetOptions,
    DeckId,
    DeckOutline,
    DeckTransaction,
    DistributeAxis,
    Element,
    ElementId,
    ElementKind,
    ElementPatch,
    ElementsApi,
    ExportOptions,
    ExportReport,
    FormulaElementInput,
    ImageElementInput,
    ImportLoss,
    InspectOptions,
    InspectReport,
    LayoutId,
    LayoutsApi,
    LockFlags,
    MastersApi,
    MediaElementInput,
    OutlineSlide,
    PlaybackApi,
    RenderOptions,
    RenderReport,
    ReplaceTextOptions,
    SaveOptions,
    SaveReport,
    SearchHitList,
    SearchOptions,
    ShapeElementInput,
    Slide,
    SlideAddOptions,
    SlideId,
    SlidesApi,
    SlideSummary,
    TableElementInput,
    TextElementInput,
    TextInput,
    TextTarget,
    ThemesApi,
    Transform,
    ZOrder,
} from './types.js';

/** Session that owns deck handles. Implemented by `OpenPPT`. */
export interface DeckOwner {
    /** Throws `session/disposed` once the session is gone. */
    assertActive(): void;
    /** Stops tracking a deck after it closed itself. */
    release(deck: Deck): void;
}

interface DeckState {
    readonly handle: number;
    summary: DeckSummary;
    readonly owner: DeckOwner;
    readonly format: PresentationFormat;
    readonly intent: DeckIntent;
    readonly importLosses: ReadonlyArray<ImportLoss>;
    closed: boolean;
}

/** An editable presentation. One instance is one deck at one revision. */
export class Deck {
    private readonly state: DeckState;
    readonly slides: SlidesApi;
    readonly masters: MastersApi;
    readonly layouts: LayoutsApi;
    readonly themes: ThemesApi;
    readonly assets: AssetsApi;
    readonly playback: PlaybackApi;

    private constructor(state: DeckState) {
        this.state = state;
        this.slides = {
            add: async (options?: SlideAddOptions) => {
                const report = this.apply([{ op: 'slide.add', layout: options?.layout, index: options?.index }]);
                const id = created(report.results, 'slide.add');
                return new SlideView(this, id);
            },
            get: async (slideId: SlideId) => new SlideView(this, slideId),
            list: async () => {
                this.refresh();
                return this.state.summary.slides.map(
                    (slide): SlideSummary => ({ id: slide.id, index: slide.index, title: slide.title, hidden: slide.hidden }),
                );
            },
            reorder: async (slideIds: readonly SlideId[]) => {
                this.apply([{ op: 'slide.reorder', slideIds }]);
            },
            move: async (slideId: SlideId, index: number) => {
                if (!Number.isInteger(index) || index < 0) {
                    throw new OpenPPTError('session/argument-invalid', '`index` must be a non-negative integer');
                }
                this.apply([{ op: 'slide.move', slideId, index }]);
            },
            duplicate: async (slideId: SlideId, options?: { readonly index?: number }) => {
                const report = this.apply([{ op: 'slide.duplicate', slideId, index: options?.index }]);
                return new SlideView(this, created(report.results, 'slide.duplicate'));
            },
            remove: async (slideId: SlideId) => {
                this.apply([{ op: 'slide.remove', slideId }]);
            },
        };
        this.masters = { list: () => unsupported('masters.list') };
        this.layouts = { list: () => unsupported('layouts.list') };
        this.themes = {
            list: () => unsupported('themes.list'),
            get: () => unsupported('themes.get'),
            apply: () => unsupported('themes.apply'),
            import: () => unsupported('themes.import'),
        };
        this.assets = {
            list: async () =>
                service.assets(this.handle('assets.list')).map(
                    (asset): AssetInfo => ({
                        id: asset.id,
                        name: asset.name,
                        mediaType: asset.mediaType,
                        bytes: asset.bytes,
                        sha256: asset.sha256,
                        ...(asset.license === null ? {} : { license: asset.license }),
                    }),
                ),
            add: async (input: AssetInput) => {
                const report = this.apply([assetAddOperation(input)]);
                const id = created(report.results, 'assets.add');
                const asset = service.assets(this.state.handle).find((candidate) => candidate.id === id);
                if (asset === undefined) {
                    throw new OpenPPTError('object/not-found', `asset \`${id}\` is missing after it was added`);
                }
                return {
                    id: asset.id,
                    name: asset.name,
                    mediaType: asset.mediaType,
                    bytes: asset.bytes,
                    sha256: asset.sha256,
                    ...(asset.license === null ? {} : { license: asset.license }),
                };
            },
            remove: async (assetId: AssetId) => {
                this.apply([{ op: 'asset.remove', assetId }]);
            },
        };
        this.playback = { create: () => unsupported('playback.create') };
    }

    /** @internal Builds the wrapper for a handle returned by the native layer. */
    static attach(
        owner: DeckOwner,
        opened: DeckHandle,
        format: PresentationFormat,
        intent: DeckIntent,
        importLosses: ReadonlyArray<ImportLoss> = [],
    ): Deck {
        return new Deck({ handle: opened.handle, summary: opened.deck, owner, format, intent, importLosses, closed: false });
    }

    /** Stable deck identity; never derived from a file name. */
    get id(): DeckId {
        return this.state.summary.id;
    }

    /** Monotonic revision, incremented on every committed transaction. */
    get revision(): number {
        return this.state.summary.revision;
    }

    /** Current slide size in points. */
    get size(): { readonly widthPt: number; readonly heightPt: number } {
        return this.state.summary.size;
    }

    /**
     * Format this deck was opened from, or `pptx` for a created deck.
     *
     * A native project has no exchange extension of its own, so it reports the
     * projection target until import metadata records the true source format.
     */
    get format(): PresentationFormat {
        return this.state.format;
    }

    /** Intent resolved at open time (`edit`, `play` or `template`). */
    get intent(): DeckIntent {
        return this.state.intent;
    }

    /**
     * Constructs the editable import could not carry.
     *
     * Empty for created and opened native projects; populated by
     * `OpenPPT.importPptx()` with stable `import.pptx.*` codes.
     */
    get importLosses(): ReadonlyArray<ImportLoss> {
        return this.state.importLosses;
    }

    /** Current committed summary: id, revision, size, layouts and slides. */
    summary(): DeckSummary {
        this.assertOpen('deck.summary');
        this.refresh();
        return this.state.summary;
    }

    /**
     * Finds visible text across every slide, without rendering.
     *
     * Matching is case-insensitive by default. With `regex: true`, `query` is a
     * JavaScript pattern whose matches are reported by offset and length.
     * `notes: true` also searches speaker notes. `wholeWord: true` only reports
     * matches delimited by non-word characters. `slideIds` restricts the
     * search to those slides, and unknown ids are ignored. `kinds` restricts
     * hits to those segment kinds, such as `['title', 'text']`; unknown kinds
     * are ignored. `maxPerSlide` caps hits per slide without setting
     * `truncated`. `limit` (default
     * 100) caps the returned hits and sets `truncated` on the result array
     * when matches were dropped. An empty query or an invalid pattern is
     * `session/argument-invalid`.
     */
    async search(query: string, options: SearchOptions = {}): Promise<SearchHitList> {
        this.assertOpen('deck.search');
        if (typeof query !== 'string' || query.trim().length === 0) {
            throw new OpenPPTError('session/argument-invalid', '`query` must be a non-empty string');
        }
        if (options.caseSensitive !== undefined && typeof options.caseSensitive !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`caseSensitive` must be a boolean');
        }
        if (options.regex !== undefined && typeof options.regex !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`regex` must be a boolean');
        }
        if (options.notes !== undefined && typeof options.notes !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`notes` must be a boolean');
        }
        if (options.wholeWord !== undefined && typeof options.wholeWord !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`wholeWord` must be a boolean');
        }
        if (
            options.slideIds !== undefined
            && (!Array.isArray(options.slideIds)
                || options.slideIds.some((id) => typeof id !== 'string' || id.trim().length === 0))
        ) {
            throw new OpenPPTError('session/argument-invalid', '`slideIds` must be an array of slide ids');
        }
        if (
            options.kinds !== undefined
            && (!Array.isArray(options.kinds)
                || options.kinds.some((kind) => typeof kind !== 'string' || kind.trim().length === 0))
        ) {
            throw new OpenPPTError('session/argument-invalid', '`kinds` must be an array of search hit kinds');
        }
        if (
            options.maxPerSlide !== undefined
            && (!Number.isInteger(options.maxPerSlide) || options.maxPerSlide < 1)
        ) {
            throw new OpenPPTError('session/argument-invalid', '`maxPerSlide` must be a positive integer');
        }
        const limit = options.limit ?? 100;
        if (!Number.isInteger(limit) || limit < 1) {
            throw new OpenPPTError('session/argument-invalid', '`limit` must be a positive integer');
        }
        this.refresh();
        const slides = this.state.summary.slides.map((slide) => service.slide(this.handle('deck.search'), slide.id));
        return searchSlides(slides, query, { ...options, limit });
    }

    /**
     * Replaces text across the deck in one atomic edit, without rendering.
     *
     * Every match of `find` inside a text run of a text element or a table
     * cell is replaced with `replace`; a match spanning two runs stays
     * untouched. Matching mirrors {@link search}: case-insensitive by default,
     * and `wholeWord: true` only replaces matches delimited by non-word
     * characters. `slideIds` restricts the rewrite to those slides, and unknown
     * ids are ignored. The returned number counts the replaced matches. An
     * empty `find` is `session/argument-invalid`.
     */
    async replaceText(find: string, replace: string, options: ReplaceTextOptions = {}): Promise<number> {
        this.assertOpen('deck.replaceText');
        if (typeof find !== 'string' || find.length === 0) {
            throw new OpenPPTError('session/argument-invalid', '`find` must be a non-empty string');
        }
        if (typeof replace !== 'string') {
            throw new OpenPPTError('session/argument-invalid', '`replace` must be a string');
        }
        if (options.caseSensitive !== undefined && typeof options.caseSensitive !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`caseSensitive` must be a boolean');
        }
        if (options.wholeWord !== undefined && typeof options.wholeWord !== 'boolean') {
            throw new OpenPPTError('session/argument-invalid', '`wholeWord` must be a boolean');
        }
        if (
            options.slideIds !== undefined
            && (!Array.isArray(options.slideIds)
                || options.slideIds.some((id) => typeof id !== 'string' || id.trim().length === 0))
        ) {
            throw new OpenPPTError('session/argument-invalid', '`slideIds` must be an array of slide ids');
        }
        const report = this.apply(
            [
                {
                    op: 'deck.replaceText',
                    find,
                    replace,
                    caseSensitive: options.caseSensitive,
                    wholeWord: options.wholeWord,
                    slideIds: options.slideIds,
                },
            ],
            options.expectedRevision,
        );
        return Number(report.results[0]);
    }

    /**
     * Lists slide titles, notes, hidden flags and element counts.
     *
     * The default outline comes from the committed summary and never triggers a
     * render. With `points: true` the body text of every slide is projected as
     * talking points, still without a render. `maxPointsPerSlide` (1 to 20,
     * default 6) only applies with `points`.
     */
    outline(options: { readonly points?: boolean; readonly maxPointsPerSlide?: number } = {}): DeckOutline {
        this.assertOpen('deck.outline');
        if (options.points !== true) {
            if (options.maxPointsPerSlide !== undefined) {
                throw new OpenPPTError('session/argument-invalid', '`maxPointsPerSlide` requires `points: true`');
            }
            this.refresh();
            return buildOutline(this.state.summary);
        }
        const report = service.outline(
            this.handle('deck.outline'),
            options.maxPointsPerSlide === undefined ? {} : { maxPointsPerSlide: options.maxPointsPerSlide },
        );
        return {
            revision: report.revision,
            slides: report.slides.map(
                (slide): OutlineSlide => ({
                    id: slide.id,
                    index: slide.index,
                    title: slide.title,
                    ...(slide.notes === null || slide.notes.length === 0 ? {} : { notes: slide.notes }),
                    hidden: slide.hidden,
                    elementCount: slide.elements,
                    points: slide.points,
                }),
            ),
        };
    }

    /**
     * Applies one atomic edit script and returns one result per operation.
     *
     * This is the same operation the CLI exposes as `--ops`; element helpers on
     * `SlideView` are convenience wrappers over it.
     */
    async edit(
        operations: ReadonlyArray<service.EditOperation>,
        options: { readonly expectedRevision?: number } = {},
    ): Promise<service.EditReport> {
        return this.apply(operations, options.expectedRevision);
    }

    /** Reads one slide with its element tree. */
    async slide(slideId: SlideId): Promise<service.SlideDetail> {
        return this.slideDetail(slideId);
    }

    /** Walks the static show and returns one frame per build step. */
    async play(): Promise<service.PlayReport> {
        this.assertOpen('deck.play');
        return service.play(this.state.handle);
    }

    /** Whether {@link close} has run. */
    get closed(): boolean {
        return this.state.closed;
    }

    /** Runs `work` atomically against one revision; a failed transaction leaves no half page. */
    async transaction<T>(work: (transaction: DeckTransaction) => Promise<T> | T): Promise<T> {
        this.assertOpen('deck.transaction');
        const transaction = new BufferedTransaction(this);
        const result = await work(transaction);
        transaction.commit();
        return result;
    }

    /** Restores the previous committed document; `false` at the beginning of history. */
    async undo(): Promise<boolean> {
        this.assertOpen('deck.undo');
        const report = service.undo(this.state.handle);
        this.state.summary = report.deck;
        return report.changed;
    }

    /** Reapplies the last undone document; `false` when redo is empty. */
    async redo(): Promise<boolean> {
        this.assertOpen('deck.redo');
        const report = service.redo(this.state.handle);
        this.state.summary = report.deck;
        return report.changed;
    }

    /** Applies rule checks (`overflow`, `contrast`, `alignment`, `assets`, `fonts`, `out-of-bounds`). */
    async inspect(options?: InspectOptions): Promise<InspectReport> {
        this.assertOpen('deck.inspect');
        return service.inspect(this.state.handle, options?.checks);
    }

    /** Renders pages to `svg`; `png` and `pdf` need a raster backend this build does not ship. */
    async render(options: RenderOptions): Promise<RenderReport> {
        this.assertOpen('deck.render');
        if (options.format !== 'svg') {
            throw new NotImplementedError(`deck.render.${options.format}`);
        }
        if (options.scale !== undefined && options.scale !== 1) {
            throw new NotImplementedError('deck.render.scale');
        }
        const report = service.render(this.state.handle, { format: 'svg' });
        let pages = report.pages;
        if (options.pages !== undefined && options.pages !== 'all') {
            const wanted = new Set(options.pages);
            pages = pages.filter((page) => wanted.has(page.index));
        }
        const directory = options.target;
        if (directory !== undefined && directory.length > 0) {
            mkdirSync(directory, { recursive: true });
            pages = pages.map((page) => {
                const path = join(directory, `slide-${page.index + 1}.svg`);
                writeFileSync(path, page.svg, 'utf8');
                return { ...page, path, bytes: Buffer.byteLength(page.svg) };
            });
        }
        return { ...report, pages };
    }

    /** Renders a whole-deck overview for visual review. */
    async contactSheet(options?: ContactSheetOptions): Promise<ContactSheet> {
        this.assertOpen('deck.contactSheet');
        if (options?.format !== undefined && options.format !== 'svg') {
            throw new NotImplementedError(`deck.contactSheet.${options.format}`);
        }
        const columns = options?.columns ?? 3;
        if (!Number.isInteger(columns) || columns < 1) {
            throw new OpenPPTError('session/argument-invalid', '`columns` must be a positive integer');
        }
        const gap = options?.gap ?? 12;
        if (!Number.isFinite(gap) || gap < 0) {
            throw new OpenPPTError('session/argument-invalid', '`gap` must be a non-negative number');
        }
        const report = await this.render({ format: 'svg', pages: options?.pages });
        const layout = buildContactSheetSvg(report.pages, { columns, gap });
        const bytes = Buffer.byteLength(layout.svg);
        const target = options?.target;
        let sheetPath: string | undefined;
        if (target !== undefined && target.length > 0) {
            mkdirSync(dirname(target), { recursive: true });
            writeFileSync(target, layout.svg, 'utf8');
            sheetPath = target;
        }
        return {
            revision: report.revision,
            slideIds: report.pages.map((page) => page.slideId),
            ...layout,
            bytes,
            ...(sheetPath === undefined ? {} : { path: sheetPath }),
        };
    }

    /** Writes a projection (`pptx`, `ppsx`, `potx` or a static `pdf`) and reports what it dropped. */
    async export(options: ExportOptions): Promise<ExportReport> {
        this.assertOpen('deck.export');
        if (options.format !== 'pptx' && options.format !== 'ppsx' && options.format !== 'potx' && options.format !== 'pdf') {
            throw new NotImplementedError(`deck.export.${options.format}`);
        }
        const target = options.target;
        if (target === undefined || target.trim().length === 0) {
            throw new OpenPPTError('session/argument-invalid', '`deck.export()` needs a target file');
        }
        if (options.lossPolicy !== undefined && options.lossPolicy !== 'warn' && options.lossPolicy !== 'error') {
            throw new OpenPPTError('session/argument-invalid', '`lossPolicy` must be `warn` or `error`');
        }
        return service.exportDeck(this.state.handle, {
            format: options.format,
            target,
            overwrite: options.overwrite ?? false,
            lossPolicy: options.lossPolicy,
        });
    }

    /** Saves the native editable project (not an exchange format). */
    async save(options?: SaveOptions): Promise<SaveReport> {
        this.assertOpen('deck.save');
        const target = options?.target;
        if (target === undefined || target.length === 0) {
            throw new OpenPPTError(
                'session/argument-invalid',
                '`deck.save()` needs a target path; a native project is always written to an explicit file',
            );
        }
        const report = service.save(this.state.handle, target, options?.overwrite ?? false);
        this.refresh();
        return { target: report.path, revision: report.revision, bytes: report.bytes };
    }

    /** Releases the deck handle. Other handles of the same session stay usable. */
    async close(): Promise<void> {
        if (this.state.closed) {
            return;
        }
        service.close(this.state.handle);
        this.state.closed = true;
        this.state.owner.release(this);
    }

    /** @internal Throws once the session or this deck is gone. */
    assertOpen(feature: string): void {
        this.state.owner.assertActive();
        if (this.state.closed) {
            throw new OpenPPTError('deck/not-found', `deck \`${this.id}\` has been closed`, { details: { feature } });
        }
    }

    /** @internal Re-reads the committed summary after a native mutation. */
    refresh(): void {
        this.state.summary = service.summary(this.state.handle);
    }

    /** @internal Marks the deck unusable after the owning session disposed. */
    markDisposed(): void {
        this.state.closed = true;
    }

    /** @internal Reads one slide with its elements. */
    slideDetail(slideId: SlideId): service.SlideDetail {
        return service.slide(this.handle('slides.get'), slideId);
    }

    /** @internal Commits one atomic script and refreshes the summary. */
    apply(operations: ReadonlyArray<service.EditOperation>, expectedRevision?: number): service.EditReport {
        this.assertOpen('deck.edit');
        const report = service.edit(this.state.handle, { expectedRevision, operations });
        this.state.summary = report.deck;
        return report;
    }

    /** @internal Sets the hidden flag synchronously; used by the writable slide property. */
    setSlideHidden(slideId: SlideId, hidden: boolean): void {
        this.apply([{ op: 'slide.setHidden', slideId, hidden }]);
    }

    /** @internal Replaces the title text; the native model keeps titles plain. */
    setSlideTitle(slideId: SlideId, text: TextInput, expectedRevision?: number): void {
        this.apply([{ op: 'slide.setTitle', slideId, text: plainText(text, 'slide.title') }], expectedRevision);
    }

    /** @internal Replaces the notes text; empty text clears the notes. */
    setSlideNotes(slideId: SlideId, text: TextInput, expectedRevision?: number): void {
        const plain = plainText(text, 'slide.notes');
        this.apply([{ op: 'slide.setNotes', slideId, text: plain.length === 0 ? null : plain }], expectedRevision);
    }

    /** @internal Handle for services that still take a raw handle. */
    handle(feature: string): number {
        this.assertOpen(feature);
        return this.state.handle;
    }
}

class SlideView implements Slide {
    constructor(
        private readonly deck: Deck,
        private readonly slideId: SlideId,
    ) {}

    get id(): SlideId {
        return this.slideId;
    }

    get index(): number {
        return this.detail().index;
    }

    get title(): TextTarget {
        return this.target('title');
    }

    get notes(): TextTarget {
        return this.target('notes');
    }

    get elements(): ElementsApi {
        return new ElementsView(this.deck, this.slideId);
    }

    get hidden(): boolean {
        return this.detail().hidden;
    }

    set hidden(value: boolean) {
        this.deck.setSlideHidden(this.slideId, value);
    }

    get layout(): LayoutId | null {
        return this.detail().layout;
    }

    set layout(value: LayoutId | null) {
        this.deck.apply([{ op: 'slide.setLayout', slideId: this.slideId, layout: value }]);
    }

    private detail(): service.SlideDetail {
        return this.deck.slideDetail(this.slideId);
    }

    private target(kind: 'title' | 'notes'): TextTarget {
        return {
            get: async () => (kind === 'title' ? this.detail().title : (this.detail().notes ?? '')),
            set: async (text: TextInput, options) => {
                if (options?.fit !== undefined && options.fit !== 'wrap') {
                    throw new OpenPPTError('object/unsupported', `\`fit: '${options.fit}'\` needs the autofit engine`, {
                        details: { feature: 'text.fit' },
                    });
                }
                if (kind === 'title') {
                    this.deck.setSlideTitle(this.slideId, text, options?.expectedRevision);
                } else {
                    this.deck.setSlideNotes(this.slideId, text, options?.expectedRevision);
                }
            },
        };
    }
}

class ElementsView {
    constructor(
        private readonly deck: Deck,
        private readonly slideId: SlideId,
    ) {}

    async addText(input: TextElementInput): Promise<Element> {
        return this.commit(textOperation(this.slideId, input));
    }

    async addImage(input: ImageElementInput): Promise<Element> {
        return this.commit({
            op: 'image.add',
            slideId: this.slideId,
            alt: input.alt,
            transform: input.transform,
            ...payloadFor(input.source, 'image'),
        });
    }

    async addShape(input: ShapeElementInput): Promise<Element> {
        return this.commit({
            op: 'shape.add',
            slideId: this.slideId,
            geometry: input.geometry,
            transform: input.transform,
            fill: input.fill,
            line: input.line,
            alt: input.alt,
        });
    }

    async addTable(input: TableElementInput): Promise<Element> {
        return this.commit(tableOperation(this.slideId, input, this.deck.size));
    }

    async addChart(input: ChartElementInput): Promise<Element> {
        return this.commit({
            op: 'chart.add',
            slideId: this.slideId,
            chartType: input.type,
            data: input.data,
            transform: input.transform,
            alt: input.alt,
        });
    }

    async addFormula(input: FormulaElementInput): Promise<Element> {
        return this.commit({
            op: 'formula.add',
            slideId: this.slideId,
            expression: input.expression,
            transform: input.transform,
            alt: input.alt,
        });
    }

    async addMedia(input: MediaElementInput): Promise<Element> {
        return this.commit({
            op: 'media.add',
            slideId: this.slideId,
            kind: input.kind,
            alt: input.alt,
            transform: input.transform,
            ...payloadFor(input.source, input.kind),
        });
    }

    async get(elementId: ElementId): Promise<Element> {
        const detail = this.deck.slideDetail(this.slideId);
        const element = detail.elements.find((candidate) => candidate.id === elementId);
        if (element === undefined) {
            throw new OpenPPTError('object/not-found', `element \`${elementId}\` is not on slide \`${this.slideId}\``, {
                details: { feature: 'elements.get', slideId: this.slideId, elementId },
            });
        }
        return elementView(element);
    }

    async update(elementId: ElementId, patch: ElementPatch): Promise<Element> {
        const operations = patchOperations(this.slideId, elementId, patch);
        if (operations.length > 0) {
            this.deck.apply(operations, patch.expectedRevision);
        }
        return this.get(elementId);
    }

    async remove(elementId: ElementId): Promise<void> {
        this.deck.apply([{ op: 'element.remove', slideId: this.slideId, elementId }]);
    }

    async group(): Promise<Element> {
        throw new OpenPPTError('object/unsupported', 'groups are not editable yet', { details: { feature: 'elements.group' } });
    }

    async align(
        elementIds: readonly ElementId[],
        edge: AlignEdge,
        options?: { readonly relativeTo?: AlignReference },
    ): Promise<void> {
        assertElementIds(elementIds, 'elements.align', 1);
        this.deck.apply([
            { op: 'element.align', slideId: this.slideId, elementIds, edge, relativeTo: options?.relativeTo },
        ]);
    }

    async distribute(elementIds: readonly ElementId[], axis: DistributeAxis): Promise<void> {
        assertElementIds(elementIds, 'elements.distribute', 3);
        this.deck.apply([{ op: 'element.distribute', slideId: this.slideId, elementIds, axis }]);
    }

    async duplicate(
        elementId: ElementId,
        options?: { readonly toSlideId?: SlideId; readonly offsetX?: number; readonly offsetY?: number },
    ): Promise<Element> {
        return this.commit({
            op: 'element.duplicate',
            slideId: this.slideId,
            elementId,
            targetSlideId: options?.toSlideId,
            offsetXPt: options?.offsetX,
            offsetYPt: options?.offsetY,
        });
    }

    async zOrder(elementId: ElementId, order: ZOrder): Promise<void> {
        const operations: Record<ZOrder, string> = {
            front: 'element.bringToFront',
            back: 'element.sendToBack',
            forward: 'element.bringForward',
            backward: 'element.sendBackward',
        };
        const operation = operations[order];
        if (operation === undefined) {
            throw new OpenPPTError('session/argument-invalid', `\`${String(order)}\` is not a stacking command`);
        }
        this.deck.apply([{ op: operation, slideId: this.slideId, elementId }]);
    }

    async lock(elementId: ElementId, flags: LockFlags): Promise<void> {
        this.deck.apply([{ op: 'element.lock', slideId: this.slideId, elementId, flags }]);
    }

    private commit(operation: service.EditOperation): Element {
        const report = this.deck.apply([operation]);
        return elementViewFrom(this.deck, this.slideId, created(report.results, String(operation.op)));
    }
}

class BufferedTransaction implements DeckTransaction {
    readonly baseRevision: number;
    private readonly operations: service.EditOperation[] = [];
    private counter = 0;

    constructor(private readonly deck: Deck) {
        this.baseRevision = deck.revision;
    }

    addSlide(options?: SlideAddOptions): SlideId {
        const token = this.token();
        this.operations.push({ op: 'slide.add', layout: options?.layout, index: options?.index, as: token });
        return token;
    }

    duplicateSlide(slideId: SlideId, options?: { readonly index?: number }): SlideId {
        const token = this.token();
        this.operations.push({ op: 'slide.duplicate', slideId, index: options?.index, as: token });
        return token;
    }

    removeSlide(slideId: SlideId): void {
        this.operations.push({ op: 'slide.remove', slideId });
    }

    moveSlide(slideId: SlideId, index: number): void {
        this.operations.push({ op: 'slide.move', slideId, index });
    }

    reorderSlides(slideIds: readonly SlideId[]): void {
        this.operations.push({ op: 'slide.reorder', slideIds });
    }

    setSlideTitle(slideId: SlideId, text: TextInput): void {
        this.operations.push({ op: 'slide.setTitle', slideId, text: plainText(text, 'slide.title') });
    }

    setSlideNotes(slideId: SlideId, text: TextInput | null): void {
        if (text === null) {
            this.operations.push({ op: 'slide.setNotes', slideId, text: null });
            return;
        }
        const plain = plainText(text, 'slide.notes');
        this.operations.push({ op: 'slide.setNotes', slideId, text: plain.length === 0 ? null : plain });
    }

    setSlideHidden(slideId: SlideId, hidden: boolean): void {
        this.operations.push({ op: 'slide.setHidden', slideId, hidden });
    }

    addText(slideId: SlideId, input: TextElementInput): ElementId {
        return this.pushCreated(textOperation(slideId, input));
    }

    addImage(slideId: SlideId, input: ImageElementInput): ElementId {
        return this.pushCreated({
            op: 'image.add',
            slideId,
            alt: input.alt,
            transform: input.transform,
            ...payloadFor(input.source, 'image'),
        });
    }

    addShape(slideId: SlideId, input: ShapeElementInput): ElementId {
        return this.pushCreated({
            op: 'shape.add',
            slideId,
            geometry: input.geometry,
            transform: input.transform,
            fill: input.fill,
            line: input.line,
            alt: input.alt,
        });
    }

    addTable(slideId: SlideId, input: TableElementInput): ElementId {
        return this.pushCreated(tableOperation(slideId, input, this.deck.size));
    }

    addChart(slideId: SlideId, input: ChartElementInput): ElementId {
        return this.pushCreated({
            op: 'chart.add',
            slideId,
            chartType: input.type,
            data: input.data,
            transform: input.transform,
            alt: input.alt,
        });
    }

    addFormula(slideId: SlideId, input: FormulaElementInput): ElementId {
        return this.pushCreated({
            op: 'formula.add',
            slideId,
            expression: input.expression,
            transform: input.transform,
            alt: input.alt,
        });
    }

    addMedia(slideId: SlideId, input: MediaElementInput): ElementId {
        return this.pushCreated({
            op: 'media.add',
            slideId,
            kind: input.kind,
            alt: input.alt,
            transform: input.transform,
            ...payloadFor(input.source, input.kind),
        });
    }

    updateElement(slideId: SlideId, elementId: ElementId, patch: ElementPatch): void {
        this.operations.push(...patchOperations(slideId, elementId, patch));
    }

    removeElement(slideId: SlideId, elementId: ElementId): void {
        this.operations.push({ op: 'element.remove', slideId, elementId });
    }

    addAsset(input: AssetInput): AssetId {
        return this.pushCreated(assetAddOperation(input));
    }

    /** @internal */
    commit(): void {
        this.deck.apply(this.operations, this.baseRevision);
    }

    private pushCreated(operation: service.EditOperation): string {
        const token = this.token();
        this.operations.push({ ...operation, as: token });
        return token;
    }

    private token(): string {
        this.counter += 1;
        return `$tx:${this.counter}`;
    }
}

function unsupported(feature: string): Promise<never> {
    return Promise.reject(new NotImplementedError(feature));
}

function created(results: ReadonlyArray<string | null>, feature: string): string {
    const id = results[0];
    if (id === undefined || id === null) {
        throw new OpenPPTError('object/not-found', `the engine did not return an identity for \`${feature}\``);
    }
    return id;
}

function elementViewFrom(deck: Deck, slideId: SlideId, elementId: ElementId): Element {
    const detail = deck.slideDetail(slideId);
    const element = detail.elements.find((candidate) => candidate.id === elementId);
    if (element === undefined) {
        throw new OpenPPTError('object/not-found', `element \`${elementId}\` is missing after the edit`, {
            details: { feature: 'elements.get', slideId, elementId },
        });
    }
    return elementView(element);
}

function elementView(element: service.ElementDetail): Element {
    const transform: Transform = {
        xPt: element.transform.xPt,
        yPt: element.transform.yPt,
        widthPt: element.transform.widthPt,
        heightPt: element.transform.heightPt,
        ...(element.transform.rotationDeg === null ? {} : { rotationDeg: element.transform.rotationDeg }),
    };
    return {
        id: element.id,
        kind: element.kind as ElementKind,
        transform,
        alt: element.alt,
        locked: element.locked,
    };
}

function assertElementIds(elementIds: readonly ElementId[], feature: string, minimum: number): void {
    if (elementIds.length < minimum) {
        throw new OpenPPTError('session/argument-invalid', `\`${feature}\` needs at least ${minimum} element ids`);
    }
    const seen = new Set<ElementId>();
    for (const id of elementIds) {
        if (typeof id !== 'string' || id.length === 0) {
            throw new OpenPPTError('session/argument-invalid', 'element ids must be non-empty strings');
        }
        if (seen.has(id)) {
            throw new OpenPPTError('session/argument-invalid', `element \`${id}\` appears twice`);
        }
        seen.add(id);
    }
}

function plainText(text: TextInput, feature: string): string {
    if (typeof text === 'string') {
        return text;
    }
    throw new OpenPPTError('object/unsupported', 'the native model stores titles and notes as plain text', {
        details: { feature: `${feature}.rich` },
    });
}

function textJson(text: TextInput): unknown {
    if (typeof text === 'string') {
        return {
            paragraphs: text.split('\n').map((line) => ({ runs: [{ text: line }], level: 0 })),
        };
    }
    return {
        paragraphs: text.paragraphs.map((paragraph) => ({
            level: paragraph.level ?? 0,
            ...(paragraph.align === undefined ? {} : { align: paragraph.align }),
            runs: paragraph.runs === undefined ? [{ text: paragraph.text ?? '' }] : paragraph.runs.map((run) => ({ ...run })),
        })),
    };
}

function textOperation(slideId: SlideId, input: TextElementInput): service.EditOperation {
    return {
        op: 'text.add',
        slideId,
        text: textJson(input.text),
        transform: input.transform,
        alt: input.alt,
        placeholder: input.placeholder,
    };
}

function tableOperation(
    slideId: SlideId,
    input: TableElementInput,
    deckSize: { readonly widthPt: number; readonly heightPt: number },
): service.EditOperation {
    const columns = input.rows[0]?.length ?? 0;
    if (columns === 0 || input.rows.length === 0) {
        throw new OpenPPTError('session/argument-invalid', 'tables need at least one column and one row');
    }
    // Without a box, tables use the body safe area and share it evenly.
    const transform: Transform = input.transform ?? {
        xPt: deckSize.widthPt * 0.1,
        yPt: deckSize.heightPt * 0.2,
        widthPt: deckSize.widthPt * 0.8,
        heightPt: deckSize.heightPt * 0.6,
    };
    const columnWidth = transform.widthPt / columns;
    const rowHeight = transform.heightPt / input.rows.length;
    return {
        op: 'table.add',
        slideId,
        transform,
        alt: input.alt,
        columnWidthsPt: Array.from({ length: columns }, () => columnWidth),
        rowHeightsPt: Array.from({ length: input.rows.length }, () => rowHeight),
        rows: input.rows.map((row) => row.map((cell) => ({ text: textJson(cell.text ?? ''), fill: cell.fill }))),
    };
}

function patchOperations(slideId: SlideId, elementId: ElementId, patch: ElementPatch): service.EditOperation[] {
    const operations: service.EditOperation[] = [];
    if (patch.transform !== undefined) {
        operations.push({ op: 'element.updateTransform', slideId, elementId, transform: patch.transform });
    }
    if (patch.alt !== undefined) {
        operations.push({ op: 'element.updateAlt', slideId, elementId, alt: patch.alt });
    }
    if (patch.text !== undefined) {
        operations.push({ op: 'element.setText', slideId, elementId, text: textJson(patch.text) });
    }
    if (patch.style !== undefined) {
        operations.push({ op: 'element.setTextStyle', slideId, elementId, ...patch.style });
    }
    if (patch.data !== undefined) {
        operations.push({ op: 'element.setChartData', slideId, elementId, data: patch.data });
    }
    if (patch.fill !== undefined) {
        operations.push({ op: 'element.setFill', slideId, elementId, fill: patch.fill });
    }
    return operations;
}

function assetAddOperation(input: AssetInput): service.EditOperation {
    const payload = payloadFor(input, 'asset');
    if (!('asset' in payload)) {
        throw new OpenPPTError('session/argument-invalid', '`assets.add()` needs `path` or `bytes`, not an identity');
    }
    return {
        op: 'asset.add',
        name: input.name,
        mediaType: payload.asset.mediaType,
        base64: payload.asset.base64,
        license: input.license,
    };
}

interface InlineAsset {
    readonly name: string;
    readonly mediaType: string;
    readonly base64: string;
}

const MEDIA_TYPES: Readonly<Record<string, string>> = {
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.gif': 'image/gif',
    '.svg': 'image/svg+xml',
    '.webp': 'image/webp',
    '.bmp': 'image/bmp',
    '.mp3': 'audio/mpeg',
    '.wav': 'audio/wav',
    '.m4a': 'audio/mp4',
    '.mp4': 'video/mp4',
    '.webm': 'video/webm',
    '.mov': 'video/quicktime',
};

function payloadFor(
    source: AssetId | BytesSource | AssetInput,
    purpose: 'image' | 'audio' | 'video' | 'asset',
): { assetId: string } | { asset: InlineAsset } {
    if (typeof source === 'string') {
        return { assetId: source };
    }
    const path = 'path' in source ? source.path : undefined;
    const explicitBytes = 'bytes' in source ? source.bytes : undefined;
    if (path === undefined && explicitBytes === undefined) {
        throw new OpenPPTError('session/argument-invalid', `adding ${purpose} needs \`path\` or \`bytes\``);
    }
    const bytes = path === undefined ? Buffer.from(explicitBytes ?? new Uint8Array()) : readFileSync(path);
    const provided = 'mediaType' in source ? source.mediaType : undefined;
    const inferred = path === undefined ? undefined : MEDIA_TYPES[extname(path).toLowerCase()];
    const mediaType = provided ?? inferred ?? (purpose === 'asset' ? 'application/octet-stream' : undefined);
    if (mediaType === undefined) {
        throw new OpenPPTError('session/argument-invalid', `adding ${purpose} bytes needs an explicit \`mediaType\``);
    }
    const name = 'name' in source ? source.name : path === undefined ? `${purpose}.${mediaType.split('/')[1] ?? 'bin'}` : basename(path);
    return { asset: { name, mediaType, base64: bytes.toString('base64') } };
}
