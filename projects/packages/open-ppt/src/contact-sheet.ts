/**
 * Contact-sheet geometry.
 *
 * Pure string and number work: renders already-produced page SVGs into one
 * document, nesting each page in its own viewport. No DOM and no renderer call,
 * so the layout is testable without the native addon.
 */

import type { ContactSheetTile, RenderPage } from './types.js';

/** The sheet fields that depend only on page SVG and grid options. */
export interface ContactSheetLayout {
    readonly columns: number;
    readonly gapPx: number;
    readonly widthPx: number;
    readonly heightPx: number;
    readonly tiles: readonly ContactSheetTile[];
    readonly svg: string;
}

/** Lays `pages` out row by row and returns one standalone SVG document. */
export function buildContactSheetSvg(
    pages: ReadonlyArray<RenderPage>,
    options: { readonly columns: number; readonly gap: number },
): ContactSheetLayout {
    const columns = options.columns;
    const gapPx = options.gap;
    const tileWidth = pages.reduce((widest, page) => Math.max(widest, page.widthPx), 0);
    const tileHeight = pages.reduce((tallest, page) => Math.max(tallest, page.heightPx), 0);
    const rows = Math.ceil(pages.length / columns);
    const widthPx = pages.length === 0 ? 0 : columns * tileWidth + (columns - 1) * gapPx;
    const heightPx = pages.length === 0 ? 0 : rows * tileHeight + (rows - 1) * gapPx;

    const tiles: ContactSheetTile[] = [];
    const frames: string[] = [];
    pages.forEach((page, position) => {
        const column = position % columns;
        const row = Math.floor(position / columns);
        const xPx = column * (tileWidth + gapPx) + (tileWidth - page.widthPx) / 2;
        const yPx = row * (tileHeight + gapPx) + (tileHeight - page.heightPx) / 2;
        tiles.push({
            index: page.index,
            slideId: page.slideId,
            xPx,
            yPx,
            widthPx: page.widthPx,
            heightPx: page.heightPx,
        });
        frames.push(
            `<svg x="${number(xPx)}" y="${number(yPx)}" width="${number(page.widthPx)}" height="${number(page.heightPx)}" viewBox="0 0 ${number(page.widthPx)} ${number(page.heightPx)}">${innerSvg(page.svg)}</svg>`,
        );
    });

    const background = `<rect width="${number(widthPx)}" height="${number(heightPx)}" fill="#ffffff"/>`;
    const svg = [
        `<svg xmlns="http://www.w3.org/2000/svg" width="${number(widthPx)}" height="${number(heightPx)}" viewBox="0 0 ${number(widthPx)} ${number(heightPx)}">`,
        background,
        ...frames,
        '</svg>',
    ].join('');

    return { columns, gapPx, widthPx, heightPx, tiles, svg };
}

/** Drops an XML prolog and the outer `<svg>` wrapper so the page can be nested. */
function innerSvg(svg: string): string {
    const start = svg.indexOf('<svg');
    if (start < 0) {
        return svg;
    }
    const openEnd = svg.indexOf('>', start);
    if (openEnd < 0) {
        return '';
    }
    const close = svg.lastIndexOf('</svg>');
    return svg.slice(openEnd + 1, close > openEnd ? close : svg.length);
}

/** Keeps generated SVG coordinates compact while staying numeric. */
function number(value: number): string {
    return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(3)));
}
