/**
 * Text extraction and matching for `deck.search()`.
 *
 * The logic here is pure: it reads slide details that already carry their
 * element text and never reaches the native layer. That keeps deck search
 * testable without an installed addon and keeps rendering out of the path.
 */

import { OpenPPTError } from './errors.js';
import type { ElementKind, SearchHit, SearchHitKind, SearchHitList, SearchOptions } from './types.js';

/** Structural slide shape accepted by {@link collectTextSegments}. */
export interface TextSourceSlide {
    readonly id: string;
    readonly index: number;
    readonly title?: string;
    readonly elements: readonly TextSourceElement[];
}

/** Structural element shape accepted by {@link collectTextSegments}. */
export interface TextSourceElement {
    readonly id?: string;
    readonly kind: string;
    readonly text?: string;
    /** Native element payload; table rows carry their cell text here. */
    readonly content?: unknown;
}

/** One searchable text unit: a slide title, a text element or a table. */
export interface TextSegment {
    readonly kind: SearchHitKind;
    readonly elementId?: string;
    readonly text: string;
}

/** One match of `query` inside `text`, in UTF-16 code units. */
export interface TextMatch {
    readonly start: number;
    readonly length: number;
}

const REGEX_SPECIALS = /[.*+?^${}()|[\]\\]/g;

function escapeForRegExp(value: string): string {
    return value.replace(REGEX_SPECIALS, '\\$&');
}

/**
 * Collects the visible text of one slide.
 *
 * The slide title comes first, then elements in paint order. Text elements
 * contribute their `text`; tables join their cells with a tab and rows with a
 * newline so a composite block is still matchable. Other kinds and alt text
 * are not visible content and stay out of the index.
 */
export function collectTextSegments(slide: TextSourceSlide): TextSegment[] {
    const segments: TextSegment[] = [];
    if (typeof slide.title === 'string' && slide.title.length > 0) {
        segments.push({ kind: 'title', text: slide.title });
    }
    for (const element of slide.elements) {
        const text = visibleText(element);
        if (text === undefined) {
            continue;
        }
        segments.push({
            kind: element.kind as ElementKind,
            ...(element.id === undefined ? {} : { elementId: element.id }),
            text,
        });
    }
    return segments;
}

/**
 * Finds every non-overlapping occurrence of `query` in `text`.
 *
 * Matching uses a Unicode-aware regular expression, so offsets stay valid in
 * the original string even when case folding would change its length. With
 * `regex`, `query` is used as the pattern instead of being escaped; an invalid
 * pattern throws `session/argument-invalid`.
 */
export function findTextMatches(
    text: string,
    query: string,
    caseSensitive = false,
    regex = false,
): TextMatch[] {
    if (text.length === 0 || query.length === 0) {
        return [];
    }
    const source = regex ? query : escapeForRegExp(query);
    let pattern: RegExp;
    try {
        pattern = new RegExp(source, caseSensitive ? 'gu' : 'giu');
    }
    catch (error) {
        throw new OpenPPTError(
            'session/argument-invalid',
            `\`query\` is not a valid regular expression: ${error instanceof Error ? error.message : String(error)}`,
        );
    }
    const matches: TextMatch[] = [];
    for (const match of text.matchAll(pattern)) {
        if (match.index === undefined || match[0].length === 0) {
            continue;
        }
        matches.push({ start: match.index, length: match[0].length });
    }
    return matches;
}

/**
 * Searches every slide in order and returns one hit per match.
 *
 * `limit` (default 100) caps the list; the returned array carries
 * `truncated: true` when matches were dropped.
 */
export function searchSlides(
    slides: readonly TextSourceSlide[],
    query: string,
    options: SearchOptions = {},
): SearchHitList {
    const caseSensitive = options.caseSensitive ?? false;
    const regex = options.regex ?? false;
    const limit = options.limit ?? 100;
    const hits: SearchHit[] = [];
    let truncated = false;
    for (const slide of slides) {
        for (const segment of collectTextSegments(slide)) {
            for (const match of findTextMatches(segment.text, query, caseSensitive, regex)) {
                if (hits.length >= limit) {
                    truncated = true;
                    break;
                }
                hits.push({
                    slideId: slide.id,
                    slideIndex: slide.index,
                    ...(segment.elementId === undefined ? {} : { elementId: segment.elementId }),
                    kind: segment.kind,
                    text: segment.text,
                    start: match.start,
                    length: match.length,
                });
            }
            if (truncated) {
                break;
            }
        }
        if (truncated) {
            break;
        }
    }
    return Object.assign(hits, { truncated });
}

function visibleText(element: TextSourceElement): string | undefined {
    if (typeof element.text === 'string' && element.text.length > 0) {
        return element.text;
    }
    const rows = tableRows(element.content);
    if (rows.length === 0) {
        return undefined;
    }
    const text = rows.map((cells) => cells.join('\t')).join('\n');
    return text.length === 0 ? undefined : text;
}

function tableRows(content: unknown): string[][] {
    if (content === null || typeof content !== 'object') {
        return [];
    }
    const rows = (content as { readonly rows?: unknown }).rows;
    if (!Array.isArray(rows)) {
        return [];
    }
    const cellsByRow: string[][] = [];
    for (const row of rows) {
        if (row === null || typeof row !== 'object') {
            continue;
        }
        const cells = (row as { readonly cells?: unknown }).cells;
        if (!Array.isArray(cells)) {
            continue;
        }
        cellsByRow.push(
            cells.map((cell) => {
                if (cell === null || typeof cell !== 'object') {
                    return '';
                }
                const text = (cell as { readonly text?: unknown }).text;
                return typeof text === 'string' ? text : '';
            }),
        );
    }
    return cellsByRow;
}
