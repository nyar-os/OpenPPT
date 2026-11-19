/**
 * Editable deck handle.
 *
 * One instance wraps one native handle. Decks come from `OpenPPT.create()` or
 * `OpenPPT.open()`; the constructor is private so no second deck model can be
 * built here. Methods that still lack a native export reject with
 * `capability/not-implemented` instead of returning fabricated data.
 */

import { NotImplementedError, OpenPPTError } from './errors.js';
import type { DeckIntent, PresentationFormat } from './formats.js';
import * as service from './service.js';
import type { DeckHandle, DeckSummary } from './service.js';
import type {
    AssetsApi,
    ContactSheet,
    ContactSheetOptions,
    DeckId,
    DeckTransaction,
    ExportOptions,
    ExportReport,
    InspectOptions,
    InspectReport,
    LayoutsApi,
    MastersApi,
    PlaybackApi,
    RenderOptions,
    RenderReport,
    SaveOptions,
    SaveReport,
    SlidesApi,
    SlideSummary,
    ThemesApi,
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
            add: () => unsupported('slides.add'),
            get: () => unsupported('slides.get'),
            list: async () => {
                this.assertOpen('slides.list');
                return this.state.summary.slides.map(
                    (slide): SlideSummary => ({
                        id: slide.id,
                        index: slide.index,
                        title: slide.title,
                        hidden: slide.hidden,
                    }),
                );
            },
            reorder: () => unsupported('slides.reorder'),
            duplicate: () => unsupported('slides.duplicate'),
            remove: () => unsupported('slides.remove'),
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
            list: () => unsupported('assets.list'),
            add: () => unsupported('assets.add'),
            remove: () => unsupported('assets.remove'),
        };
        this.playback = { create: () => unsupported('playback.create') };
    }

    /** @internal Builds the wrapper for a handle returned by the native layer. */
    static attach(owner: DeckOwner, opened: DeckHandle, format: PresentationFormat, intent: DeckIntent): Deck {
        return new Deck({ handle: opened.handle, summary: opened.deck, owner, format, intent, closed: false });
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

    /** Whether {@link close} has run. */
    get closed(): boolean {
        return this.state.closed;
    }

    /** Runs `work` atomically against one revision; a failed transaction leaves no half page. */
    async transaction<T>(_work: (transaction: DeckTransaction) => Promise<T> | T): Promise<T> {
        this.assertOpen('deck.transaction');
        throw new NotImplementedError('deck.transaction');
    }

    /** Applies rule checks (`overflow`, `contrast`, `alignment`, `assets`, `fonts`, `out-of-bounds`). */
    async inspect(options?: InspectOptions): Promise<InspectReport> {
        this.assertOpen('deck.inspect');
        return service.inspect(this.state.handle, options?.checks);
    }

    /** Renders pages to `svg`, `png` or `pdf`. */
    async render(_options: RenderOptions): Promise<RenderReport> {
        this.assertOpen('deck.render');
        throw new NotImplementedError('deck.render');
    }

    /** Renders a whole-deck overview for visual review. */
    async contactSheet(_options?: ContactSheetOptions): Promise<ContactSheet> {
        this.assertOpen('deck.contactSheet');
        throw new NotImplementedError('deck.contactSheet');
    }

    /** Writes a projection (`pptx`, `ppsx`, `potx`, `pdf`, `odp`) and reports losses. */
    async export(_options: ExportOptions): Promise<ExportReport> {
        this.assertOpen('deck.export');
        throw new NotImplementedError('deck.export');
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
}

function unsupported(feature: string): Promise<never> {
    return Promise.reject(new NotImplementedError(feature));
}
