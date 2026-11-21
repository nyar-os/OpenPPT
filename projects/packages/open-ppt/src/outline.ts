/**
 * Deck outline built from the committed summary.
 *
 * The summary already carries each slide's title, hidden flag, element count
 * and notes, so an outline never needs a render or a second native call.
 */

import type { DeckOutline, OutlineSlide } from './types.js';

/** Structural summary slide accepted by {@link buildOutline}. */
export interface OutlineSourceSlide {
    readonly id: string;
    readonly index: number;
    readonly title: string;
    readonly hidden: boolean;
    readonly elements: number;
    readonly notes: string | null;
}

/** Structural summary accepted by {@link buildOutline}. */
export interface OutlineSource {
    readonly revision: number;
    readonly slides: readonly OutlineSourceSlide[];
}

/** Maps a committed deck summary onto the public outline shape. */
export function buildOutline(source: OutlineSource): DeckOutline {
    return {
        revision: source.revision,
        slides: source.slides.map(
            (slide): OutlineSlide => ({
                id: slide.id,
                index: slide.index,
                title: slide.title,
                ...(slide.notes === null || slide.notes.length === 0 ? {} : { notes: slide.notes }),
                hidden: slide.hidden,
                elementCount: slide.elements,
            }),
        ),
    };
}
