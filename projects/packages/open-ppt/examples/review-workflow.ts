/**
 * Per-slide review and revision workflow, the runnable form of
 * `documentation/agents.md`.
 *
 * The script builds a two-slide deck, drives one element off the canvas, runs
 * a deck-wide inspection covering the whole-deck, the no-object and the
 * second-slide findings, fixes only the reported objects, re-checks the
 * revision, writes a reviewable SVG preview and a contact sheet, exports a
 * PPTX reporting its losses, saves the native project and reopens it to prove
 * the edited element is still addressable and editable. It exits non-zero on
 * the first failed assertion, so CI runs the documented workflow verbatim
 * instead of trusting the prose.
 *
 * Run from the repository root with a built binding installed or staged:
 *
 * ```bash
 * pnpm --filter @wae/open-ppt run build:test
 * node projects/packages/open-ppt/dist-test/examples/review-workflow.js
 * ```
 *
 * On hosts without an installed platform package point `OPEN_PPT_NATIVE` at a
 * freshly built addon (`target/debug/open_ppt_napi.dll` or `.so`).
 */

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { OpenPPT } from '../src/index.js';
import { nativeBinding } from '../src/native.js';

async function main(): Promise<void> {
    if (nativeBinding() === undefined) {
        throw new Error('this example needs a built native binding; install the platform package or set OPEN_PPT_NATIVE');
    }
    const dir = mkdtempSync(join(tmpdir(), 'open-ppt-review-'));
    const pptxPath = join(dir, 'q4.pptx');
    const projectPath = join(dir, 'q4.oppt');

    // 1. Build the deck: a compliant first slide and one element pushed past
    //    the right edge of the 960pt canvas on the second slide.
    const ppt = new OpenPPT();
    const deck = await ppt.create({ title: 'Q4 review', size: { widthPt: 960, heightPt: 540 }, slides: ['Highlights', 'Revenue'] });
    const [firstId, secondId] = deck.summary().slides.map((slide) => slide.id);
    assert.ok(firstId !== undefined && secondId !== undefined, 'create returns both slide ids');
    const first = await deck.slides.get(firstId);
    const second = await deck.slides.get(secondId);

    await first.title.set('Q4 highlights');
    await first.notes.set('Lead with the 18% revenue lift.');
    await first.elements.addText({
        text: 'Revenue is up 18%',
        transform: { xPt: 72, yPt: 140, widthPt: 480, heightPt: 90 },
    });
    const revenueText = await second.elements.addText({
        text: 'Plan: expand to two regions in Q1',
        transform: { xPt: 720, yPt: 420, widthPt: 480, heightPt: 90 },
    });

    // 2. Deck-wide inspection. `overflow` is not wired in this build, so the
    //    report carries a `deck.check-unsupported` finding with neither
    //    `slideId` nor `elementId`; the out-of-bounds finding addresses the
    //    second slide. Guard on both ids before repairing.
    const inspection = await deck.inspect({ checks: ['out-of-bounds', 'contrast', 'overflow'] });
    const unaddressable = inspection.issues.filter((issue) => issue.slideId === undefined || issue.elementId === undefined);
    assert.ok(unaddressable.every((issue) => issue.code === 'deck.check-unsupported'), 'only unsupported checks report without an address');
    const addressed = inspection.issues.filter((issue) => issue.slideId !== undefined && issue.elementId !== undefined);
    assert.ok(addressed.some((issue) => issue.slideId === secondId), 'the off-canvas element is diagnosed on the second slide');
    assert.ok(!addressed.some((issue) => issue.slideId === firstId && issue.code === 'deck.out-of-bounds'), 'the compliant first slide is clean');

    // 3. Fix only the reported objects on their own slides.
    for (const issue of addressed) {
        const slideId = issue.slideId;
        const elementId = issue.elementId;
        if (slideId === undefined || elementId === undefined) {
            continue;
        }
        const slide = await deck.slides.get(slideId);
        await slide.elements.update(elementId, { transform: { xPt: 72, yPt: 140, widthPt: 480, heightPt: 90 } });
    }

    // 4. Re-check the revision the fixes were committed to.
    const recheck = await deck.inspect({ checks: ['out-of-bounds'] });
    assert.equal(recheck.revision, deck.summary().revision, 'the re-check sees the committed fixes');
    assert.ok(!recheck.issues.some((issue) => issue.slideId === secondId), 'the repaired element no longer breaches the canvas');

    // 5. Reviewable preview: two standalone SVG pages plus a contact sheet.
    const preview = await deck.render({ format: 'svg', target: join(dir, 'preview') });
    assert.equal(preview.pages.length, 2, 'the preview holds every slide');
    assert.ok(preview.pages.every((page) => page.svg.startsWith('<svg')), 'each page is a standalone SVG document');
    const sheet = await deck.contactSheet({ target: join(dir, 'review.svg'), pages: [0, 1] });
    assert.ok(existsSync(join(dir, 'review.svg')), 'the contact sheet lands on disk');
    assert.equal(sheet.slideIds.length, 2, 'the sheet holds only the requested slides');

    // 6. Export reports its losses instead of claiming fidelity.
    const exported = await deck.export({ format: 'pptx', target: pptxPath, overwrite: true });
    assert.ok(Array.isArray(exported.losses), 'the export reports a loss list');
    assert.ok(exported.bytes > 0, 'the PPTX is not empty');
    const saved = await deck.save({ target: projectPath, overwrite: true });
    assert.equal(saved.revision, deck.summary().revision);
    await deck.close();

    // 7. Reopen the native project: the edited element is still addressable.
    const reopened = await ppt.open({ path: projectPath });
    assert.equal(reopened.summary().revision, saved.revision);
    const [, revenue] = await reopened.slides.list();
    assert.ok(revenue !== undefined && revenue.id === secondId, 'the second slide keeps its identity');
    const restored = await (await reopened.slides.get(revenue.id)).elements.get(revenueText.id);
    assert.equal(restored.transform.xPt, 72, 'the repair survives the round trip');
    assert.equal(restored.transform.widthPt, 480);
    await reopened.close();

    console.log(`review workflow ok: ${addressed.length} findings repaired, pptx ${exported.bytes} bytes, project revision ${saved.revision}`);
}

await main();
