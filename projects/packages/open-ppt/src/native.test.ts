/**
 * Native integration checks.
 *
 * These run only when an addon actually resolves: either the `OPEN_PPT_NATIVE`
 * override or the installed `@wae/open-ppt-<platform>-<arch>` package.
 */

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { isOpenPPTError } from './errors.js';
import { nativeBinding } from './native.js';
import { OpenPPT } from './open-ppt.js';

const installed = (() => {
    try {
        return nativeBinding() !== undefined;
    } catch {
        return false;
    }
})();
const skip = installed ? false : 'no native addon is resolvable';

test('capabilities mirror the native matrix instead of a local guess', { skip }, async () => {
    const ppt = new OpenPPT();
    const capabilities = await ppt.capabilities();
    const matrix = capabilities as unknown as {
        project: { import: string; playback: string };
        formats: { pptx: { import: string; edit: string } };
        checks: { contrast: string; overflow: string };
        renderers: { svg: string; png: string };
        exportTargets: { pptx: string; ppsx: string; pdf: string };
        exportLossCodes: { pptx: readonly string[] };
        playback: { staticSlides: string };
    };

    assert.equal(matrix.project.import, 'supported');
    assert.equal(matrix.project.playback, 'supported');
    assert.equal(matrix.playback.staticSlides, 'supported');
    assert.equal(matrix.checks.contrast, 'supported');
    assert.equal(matrix.checks.overflow, 'unsupported');
    assert.equal(matrix.formats.pptx.import, 'partial');
    assert.equal(matrix.formats.pptx.edit, 'partial');
    assert.equal(matrix.renderers.svg, 'supported');
    assert.equal(matrix.renderers.png, 'unsupported');
    assert.equal(matrix.exportTargets.pptx, 'partial');
    assert.equal(matrix.exportTargets.ppsx, 'partial');
    assert.equal(matrix.exportTargets.pdf, 'partial');
    assert.ok(!matrix.exportLossCodes.pptx.includes('table'), 'tables are no longer an export loss');
});

test('create, save and reopen keep one handle per deck', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create({ title: 'Status', size: { widthPt: 960, heightPt: 540 } });

    assert.equal(deck.revision, 0);
    assert.equal(deck.size.widthPt, 960);
    assert.deepEqual(await deck.slides.list(), []);

    const dir = mkdtempSync(join(tmpdir(), 'open-ppt-'));
    const target = join(dir, 'status.project');
    const report = await deck.save({ target });
    assert.equal(report.target, target);
    assert.ok(report.bytes > 0);

    const reopened = await ppt.open({ path: target });
    assert.equal(reopened.id, deck.id);
    assert.equal(reopened.size.heightPt, 540);
    await reopened.close();
    await assert.rejects(
        reopened.slides.list(),
        (error) => isOpenPPTError(error) && error.code === 'deck/not-found',
    );

    await ppt.dispose();
    await assert.rejects(
        deck.save({ target: join(dir, 'again.project') }),
        (error) => isOpenPPTError(error) && error.code === 'session/disposed',
    );
});

test('editing a slide commits once per operation and survives save/reopen', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create({ title: 'Edits' });
    const slide = await deck.slides.add();
    assert.equal(deck.revision, 1);
    await slide.title.set('Metrics');
    const text = await slide.elements.addText({
        text: { paragraphs: [{ runs: [{ text: 'Hello', bold: true }] }] },
        transform: { xPt: 60, yPt: 60, widthPt: 300, heightPt: 80 },
        alt: 'Greeting',
    });
    const shape = await slide.elements.addShape({
        geometry: 'rect',
        transform: { xPt: 60, yPt: 160, widthPt: 200, heightPt: 100 },
        fill: '#ff0000',
    });
    const table = await slide.elements.addTable({
        rows: [[{ text: 'A' }, { text: 'B' }], [{ text: '1' }, { text: '2' }]],
        transform: { xPt: 300, yPt: 60, widthPt: 240, heightPt: 80 },
    });
    const chart = await slide.elements.addChart({
        type: 'column',
        data: { categories: ['Q1', 'Q2'], series: [{ name: 'Sales', values: [1, 2] }] },
        alt: 'Sales',
        transform: { xPt: 300, yPt: 180, widthPt: 240, heightPt: 140 },
    });
    const formula = await slide.elements.addFormula({ expression: 'x^2', alt: 'Square' });
    assert.deepEqual(
        [text.kind, shape.kind, table.kind, chart.kind, formula.kind],
        ['text', 'shape', 'table', 'chart', 'formula'],
    );
    await slide.notes.set('Speaker notes');
    assert.equal(deck.revision, 8);

    const dir = mkdtempSync(join(tmpdir(), 'open-ppt-'));
    const target = join(dir, 'edits.project');
    await deck.save({ target });

    const reopened = await ppt.open({ path: target });
    const [summary] = await reopened.slides.list();
    assert.ok(summary);
    const restored = await reopened.slides.get(summary.id);
    assert.equal(await restored.title.get(), 'Metrics');
    assert.equal(await restored.notes.get(), 'Speaker notes');
    const element = await restored.elements.get(text.id);
    assert.equal(element.alt, 'Greeting');
    assert.equal(element.transform.widthPt, 300);
    await reopened.close();
    await ppt.dispose();
});

test('transactions commit atomically and leave no half page on failure', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create();
    await deck.transaction((transaction) => {
        const slide = transaction.addSlide();
        transaction.setSlideTitle(slide, 'Agenda');
        const element = transaction.addText(slide, { text: 'First item' });
        transaction.updateElement(slide, element, { alt: 'first item' });
    });
    assert.equal((await deck.slides.list()).length, 1);
    const [summary] = await deck.slides.list();
    assert.ok(summary);
    assert.equal(await (await deck.slides.get(summary.id)).title.get(), 'Agenda');

    const revision = deck.revision;
    await assert.rejects(
        deck.transaction((transaction) => {
            transaction.addSlide();
            throw new Error('stop before commit');
        }),
        /stop before commit/,
    );
    assert.equal(deck.revision, revision);
    assert.equal((await deck.slides.list()).length, 1);

    await assert.rejects(
        (await deck.slides.get(summary.id)).title.set('Stale', { expectedRevision: 0 }),
        (error) => isOpenPPTError(error) && error.code === 'deck/revision-conflict',
    );
    await ppt.dispose();
});

test('undo, redo and embedded assets stay consistent', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create();
    await deck.slides.add();
    assert.equal((await deck.slides.list()).length, 1);
    assert.equal(await deck.undo(), true);
    assert.equal((await deck.slides.list()).length, 0);
    assert.equal(await deck.redo(), true);
    assert.equal((await deck.slides.list()).length, 1);

    const png = Uint8Array.from(
        Buffer.from(
            'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
            'base64',
        ),
    );
    const asset = await deck.assets.add({ name: 'dot.png', bytes: png, mediaType: 'image/png' });
    assert.equal(asset.bytes, png.length);
    const [summary] = await deck.slides.list();
    assert.ok(summary);
    const slide = await deck.slides.get(summary.id);
    const image = await slide.elements.addImage({ source: asset.id, alt: 'Dot' });
    assert.equal(image.kind, 'image');
    assert.equal((await slide.elements.get(image.id)).alt, 'Dot');
    assert.equal((await deck.assets.list()).length, 1);
    await ppt.dispose();
});

test('render and export answer with real pages and a real package', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create({ title: 'Render', size: { widthPt: 960, heightPt: 540 } });
    const slide = await deck.slides.add();
    await slide.title.set('Welcome');
    await slide.elements.addText({
        text: {
            paragraphs: [{ runs: [{ text: 'Hello <OpenPPT>', bold: true, sizePt: 24, color: '#ff0000' }], align: 'center' }],
        },
        transform: { xPt: 60, yPt: 60, widthPt: 400, heightPt: 80 },
        alt: 'Greeting',
    });
    await slide.elements.addShape({
        geometry: 'ellipse',
        transform: { xPt: 60, yPt: 180, widthPt: 200, heightPt: 120 },
        fill: '#00ff00',
    });

    const dir = mkdtempSync(join(tmpdir(), 'open-ppt-render-'));
    const render = await deck.render({ format: 'svg', target: dir });
    assert.equal(render.format, 'svg');
    assert.equal(render.pages.length, 1);
    const [page] = render.pages;
    assert.ok(page);
    assert.equal(page.index, 0);
    assert.equal(Math.round(page.widthPx), 1280);
    assert.equal(Math.round(page.heightPx), 720);
    assert.ok(page.svg.startsWith('<svg'));
    assert.ok(page.svg.includes('Hello &lt;OpenPPT&gt;'), 'text must be XML escaped');
    assert.ok(page.svg.includes('<ellipse'));
    assert.ok(page.path !== undefined && existsSync(page.path));
    assert.equal(page.bytes, Buffer.byteLength(page.svg));

    const target = join(dir, 'deck.pptx');
    const exported = await deck.export({ format: 'pptx', target });
    assert.equal(exported.format, 'pptx');
    assert.equal(exported.target, target);
    assert.equal(exported.editable, true);
    assert.ok(exported.bytes > 0);
    assert.ok(existsSync(target));

    const pdfTarget = join(dir, 'deck.pdf');
    const pdf = await deck.export({ format: 'pdf', target: pdfTarget });
    assert.equal(pdf.format, 'pdf');
    assert.equal(pdf.target, pdfTarget);
    assert.equal(pdf.editable, false);
    assert.ok(pdf.bytes > 0);
    assert.ok(readFileSync(pdfTarget).subarray(0, 8).toString('latin1').startsWith('%PDF-1.7'));
    assert.ok(existsSync(pdfTarget));
    await ppt.dispose();
});

test('exported packages import back as editable decks', { skip }, async () => {
    const ppt = new OpenPPT();
    const deck = await ppt.create({ title: 'Import', size: { widthPt: 960, heightPt: 540 } });
    const slide = await deck.slides.add();
    await slide.title.set('Plan');
    await slide.elements.addText({
        text: { paragraphs: [{ runs: [{ text: 'Hello import', bold: true }] }] },
        transform: { xPt: 60, yPt: 80, widthPt: 380, heightPt: 80 },
        alt: 'Greeting',
    });
    await slide.elements.addTable({
        rows: [[{ text: 'A' }, { text: 'B' }], [{ text: '1' }, { text: '2' }]],
        transform: { xPt: 60, yPt: 200, widthPt: 240, heightPt: 80 },
    });

    const dir = mkdtempSync(join(tmpdir(), 'open-ppt-import-'));
    const target = join(dir, 'import.pptx');
    await deck.export({ format: 'pptx', target });

    const imported = await ppt.importPptx(target);
    assert.equal(imported.format, 'pptx');
    assert.equal(imported.intent, 'edit');
    assert.equal(imported.size.widthPt, 960);
    assert.equal(imported.size.heightPt, 540);
    assert.equal(
        imported.importLosses.filter((loss) => loss.slide !== null).length,
        0,
        'text, pictures and tables import without slide-scoped content losses',
    );

    const slides = await imported.slides.list();
    assert.equal(slides.length, 1);
    assert.ok(slides[0]);
    assert.equal(await (await imported.slides.get(slides[0].id)).title.get(), 'Plan');

    const again = join(dir, 'import-again.pptx');
    const exported = await imported.export({ format: 'pptx', target: again });
    assert.ok(exported.bytes > 0);

    await assert.rejects(
        ppt.importPptx(''),
        (error) => isOpenPPTError(error) && error.code === 'session/argument-invalid',
    );
    await ppt.dispose();
});

test('unwired inputs keep stable codes instead of inventing a deck', { skip }, async () => {
    const ppt = new OpenPPT();
    await assert.rejects(
        ppt.create({ size: { widthPt: 0, heightPt: 540 } }),
        (error) => isOpenPPTError(error) && error.code === 'session/argument-invalid',
    );
    await assert.rejects(
        ppt.open({ bytes: new Uint8Array() }),
        (error) => isOpenPPTError(error) && error.code === 'format/unsupported',
    );
    await assert.rejects(
        ppt.open({ project: '{}' }),
        (error) => isOpenPPTError(error) && error.code === 'format/unsupported',
    );
    await ppt.dispose();
});
