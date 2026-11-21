/**
 * Smoke tests for the declared authoring surface.
 *
 * They assert shell semantics only: one stable error code for unimplemented
 * capabilities, a session that closes idempotently, synchronous argument
 * validation, and a format registry that classifies the whole family. No deck,
 * renderer or import behaviour is claimed here.
 */

import assert from 'node:assert/strict';
import test from 'node:test';

import { formatFromPath, formatInfo, OpenPPT, OpenPPTError, presentationFormats } from './index.js';
import { buildOutline, collectTextSegments, findTextMatches, searchSlides } from './index.js';
import type { DeckInput } from './index.js';
import { buildContactSheetSvg } from './contact-sheet.js';
import { nativeBinding } from './native.js';

const installed = (() => {
    try {
        return nativeBinding() !== undefined;
    } catch {
        return false;
    }
})();
const withAddon = installed ? 'a built addon is installed' : false;

test('session construction is synchronous and side-effect free', async () => {
    const ppt = new OpenPPT();
    assert.equal(ppt.locale, 'en');
    assert.equal(ppt.disposed, false);
    await ppt.dispose();
    assert.equal(ppt.disposed, true);
});

test('without an installed addon engine calls keep capability/not-implemented', { skip: withAddon }, async () => {
    const ppt = new OpenPPT({ locale: 'zh-CN' });
    assert.equal(ppt.locale, 'zh-CN');
    const calls: (() => Promise<unknown>)[] = [
        () => ppt.capabilities(),
        () => ppt.create({ title: 'Research review', size: '16:9' }),
        () => ppt.open({ path: 'deck.pptx' }),
        () => ppt.inspect({ path: 'deck.ppsx' }),
    ];
    for (const call of calls) {
        await assert.rejects(call, (error: unknown) => {
            assert.ok(error instanceof OpenPPTError);
            assert.equal(error.code, 'capability/not-implemented');
            assert.ok(error.details.feature);
            return true;
        });
    }
    await ppt.dispose();
});

test('dispose is idempotent and closes the session', async () => {
    const ppt = new OpenPPT();
    await ppt.dispose();
    await ppt.dispose();
    await assert.rejects(ppt.capabilities(), (error: unknown) => error instanceof OpenPPTError && error.code === 'session/disposed');
});

test('open rejects ambiguous input before reaching the engine', async () => {
    const ppt = new OpenPPT();
    const inputs = [
        {} as unknown as DeckInput,
        { path: 'a.pptx', bytes: new Uint8Array() } as unknown as DeckInput,
        { bytes: 'not bytes' } as unknown as DeckInput,
        { path: '' } as unknown as DeckInput,
    ];
    for (const input of inputs) {
        await assert.rejects(
            ppt.open(input),
            (error: unknown) => error instanceof OpenPPTError && error.code === 'session/argument-invalid',
        );
    }
    await ppt.dispose();
});

test('format registry classifies the presentation format family', () => {
    const expected = [
        'pptx',
        'pptm',
        'ppsx',
        'ppsm',
        'potx',
        'potm',
        'thmx',
        'ppam',
        'ppa',
        'ppt',
        'pps',
        'pot',
        'xml',
        'odp',
        'pdf',
        'xps',
        'mp4',
        'wmv',
        'png',
        'jpg',
        'jpeg',
        'gif',
        'tif',
        'tiff',
    ];
    assert.deepEqual([...presentationFormats].sort(), [...expected].sort());

    assert.equal(formatInfo('ppsx').defaultIntent, 'play');
    assert.equal(formatInfo('ppsm').defaultIntent, 'play');
    assert.equal(formatInfo('ppsm').macroEnabled, true);
    assert.equal(formatInfo('pptm').macroEnabled, true);
    assert.equal(formatInfo('potx').defaultIntent, 'template');
    assert.equal(formatInfo('thmx').opensAs, 'theme');
    assert.equal(formatInfo('thmx').defaultIntent, null);
    assert.equal(formatInfo('ppam').opensAs, 'none');
    assert.equal(formatInfo('ppt').family, 'legacy');
    assert.equal(formatInfo('odp').family, 'document');

    assert.equal(formatFromPath('Quarterly.PPSX'), 'ppsx');
    assert.equal(formatFromPath('slides.key'), undefined);
    assert.equal(formatFromPath('no-extension'), undefined);
});

test('contact sheet nests page SVGs on a grid without the native addon', () => {
    const page = (index: number) => ({
        index,
        slideId: `slide-${index}`,
        widthPx: 160,
        heightPx: 90,
        svg: `<?xml version="1.0" encoding="UTF-8"?><svg xmlns="http://www.w3.org/2000/svg" width="160" height="90" viewBox="0 0 160 90"><rect width="160" height="90" fill="#${index}"/></svg>`,
    });
    const pages = [page(0), page(1), page(2), page(3)];

    const sheet = buildContactSheetSvg(pages, { columns: 2, gap: 10 });
    assert.equal(sheet.widthPx, 330);
    assert.equal(sheet.heightPx, 190);
    assert.deepEqual(
        sheet.tiles.map((tile) => [tile.index, tile.xPx, tile.yPx]),
        [
            [0, 0, 0],
            [1, 170, 0],
            [2, 0, 100],
            [3, 170, 100],
        ],
    );
    assert.equal(sheet.svg.match(/<svg /g)?.length, 5);
    assert.equal(sheet.svg.includes('<?xml'), false);
    assert.equal(sheet.svg.includes('viewBox="0 0 160 90"'), true);
    assert.equal(sheet.svg.includes('fill="#3"'), true);

    const threeColumns = buildContactSheetSvg(pages, { columns: 3, gap: 12 });
    assert.equal(threeColumns.widthPx, 504);
    assert.equal(threeColumns.heightPx, 192);
    assert.equal(buildContactSheetSvg([], { columns: 3, gap: 12 }).widthPx, 0);
});

test('text segments cover titles, text elements and table cells', () => {
    const segments = collectTextSegments({
        id: 'slide-1',
        index: 0,
        title: 'Quarterly review',
        elements: [
            { id: 'text-1', kind: 'text', text: 'Revenue grew 12%' },
            {
                id: 'table-1',
                kind: 'table',
                content: {
                    rows: [{ cells: [{ text: 'North' }, { text: '120' }] }, { cells: [{ text: 'South' }, { text: '80' }] }],
                },
            },
            { id: 'image-1', kind: 'image', content: { assetId: 'asset-1' } },
            { id: 'shape-1', kind: 'shape', text: '' },
        ],
    });

    assert.deepEqual(
        segments.map((segment) => [segment.kind, segment.elementId ?? null, segment.text]),
        [
            ['title', null, 'Quarterly review'],
            ['text', 'text-1', 'Revenue grew 12%'],
            ['table', 'table-1', 'North\t120\nSouth\t80'],
        ],
    );
});

test('matching honours case, offsets and literal query text', () => {
    assert.deepEqual(findTextMatches('Revenue revenue REVENUE', 'revenue'), [
        { start: 0, length: 7 },
        { start: 8, length: 7 },
        { start: 16, length: 7 },
    ]);
    assert.deepEqual(findTextMatches('Revenue revenue REVENUE', 'revenue', true), [{ start: 8, length: 7 }]);
    assert.deepEqual(findTextMatches('1.5 and 125', '1.5'), [{ start: 0, length: 3 }]);
    assert.deepEqual(findTextMatches('plain', 'missing'), []);
});

test('deck search walks slides in order and truncates at the limit', () => {
    const slides = [
        {
            id: 'slide-1',
            index: 0,
            title: 'Revenue overview',
            elements: [
                { id: 'text-1', kind: 'text', text: 'Revenue is up' },
                { id: 'table-1', kind: 'table', content: { rows: [{ cells: [{ text: 'Revenue' }, { text: '42' }] }] } },
            ],
        },
        {
            id: 'slide-2',
            index: 1,
            title: 'Plan',
            elements: [{ id: 'text-2', kind: 'text', text: 'revenue target' }],
        },
    ];

    const hits = searchSlides(slides, 'REVENUE');
    assert.equal(hits.length, 4);
    assert.equal(hits.truncated, false);
    assert.deepEqual(
        hits.map((hit) => [hit.slideId, hit.slideIndex, hit.elementId ?? null, hit.kind]),
        [
            ['slide-1', 0, null, 'title'],
            ['slide-1', 0, 'text-1', 'text'],
            ['slide-1', 0, 'table-1', 'table'],
            ['slide-2', 1, 'text-2', 'text'],
        ],
    );
    assert.equal(hits[2]?.start, 0);
    assert.equal(hits[2]?.text, 'Revenue\t42');

    const limited = searchSlides(slides, 'revenue', { limit: 2 });
    assert.equal(limited.length, 2);
    assert.equal(limited.truncated, true);

    const caseSensitive = searchSlides(slides, 'Revenue', { caseSensitive: true });
    assert.equal(caseSensitive.length, 3);
    assert.equal(caseSensitive.some((hit) => hit.slideId === 'slide-2'), false);
    assert.equal(searchSlides(slides, 'absent').length, 0);
});

test('outline maps the committed summary without rendering', () => {
    const outline = buildOutline({
        revision: 3,
        slides: [
            { id: 'slide-1', index: 0, title: 'Intro', hidden: false, elements: 2, notes: null },
            { id: 'slide-2', index: 1, title: 'Plan', hidden: true, elements: 0, notes: 'Say hi' },
        ],
    });

    assert.equal(outline.revision, 3);
    assert.deepEqual(outline.slides, [
        { id: 'slide-1', index: 0, title: 'Intro', hidden: false, elementCount: 2 },
        { id: 'slide-2', index: 1, title: 'Plan', notes: 'Say hi', hidden: true, elementCount: 0 },
    ]);
    assert.equal('notes' in (outline.slides[0] ?? {}), false);
});
