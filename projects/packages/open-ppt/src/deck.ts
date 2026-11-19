/**
 * Editable deck handle.
 *
 * Declared surface only: every method rejects with
 * `capability/not-implemented` until the shared engine lands. The class is not
 * constructible from user code; decks come from `OpenPPT.create()` or
 * `OpenPPT.open()`, and no second deck model is introduced here.
 */

import { NotImplementedError } from './errors.js';
import type { DeckIntent, PresentationFormat } from './formats.js';
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
    ThemesApi,
} from './types.js';

/** An editable presentation. One instance is one deck at one revision. */
export class Deck {
    private constructor() {
        throw new NotImplementedError('deck');
    }

    /** Stable deck identity; never derived from a file name. */
    declare readonly id: DeckId;
    /** Monotonic revision, incremented on every committed transaction. */
    declare readonly revision: number;
    /** Current slide size in points. */
    declare readonly size: { readonly widthPt: number; readonly heightPt: number };
    /** Format this deck was opened from, or `pptx` for a created deck. */
    declare readonly format: PresentationFormat;
    /** Intent resolved at open time (`edit`, `play` or `template`). */
    declare readonly intent: DeckIntent;
    declare readonly slides: SlidesApi;
    declare readonly masters: MastersApi;
    declare readonly layouts: LayoutsApi;
    declare readonly themes: ThemesApi;
    declare readonly assets: AssetsApi;
    declare readonly playback: PlaybackApi;

    /** Runs `work` atomically against one revision; a failed transaction leaves no half page. */
    async transaction<T>(_work: (transaction: DeckTransaction) => Promise<T> | T): Promise<T> {
        throw new NotImplementedError('deck.transaction');
    }

    /** Applies rule checks (`overflow`, `contrast`, `alignment`, `assets`, `fonts`, `out-of-bounds`). */
    async inspect(_options?: InspectOptions): Promise<InspectReport> {
        throw new NotImplementedError('deck.inspect');
    }

    /** Renders pages to `svg`, `png` or `pdf`. */
    async render(_options: RenderOptions): Promise<RenderReport> {
        throw new NotImplementedError('deck.render');
    }

    /** Renders a whole-deck overview for visual review. */
    async contactSheet(_options?: ContactSheetOptions): Promise<ContactSheet> {
        throw new NotImplementedError('deck.contactSheet');
    }

    /** Writes a projection (`pptx`, `ppsx`, `potx`, `pdf`, `odp`) and reports losses. */
    async export(_options: ExportOptions): Promise<ExportReport> {
        throw new NotImplementedError('deck.export');
    }

    /** Saves the native editable project (not an exchange format). */
    async save(_options?: SaveOptions): Promise<SaveReport> {
        throw new NotImplementedError('deck.save');
    }

    /** Releases the deck handle. Other handles of the same session stay usable. */
    async close(): Promise<void> {
        throw new NotImplementedError('deck.close');
    }
}
