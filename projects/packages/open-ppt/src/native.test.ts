/**
 * Native integration checks.
 *
 * These only run when `OPEN_PPT_NATIVE` points at a built addon; the default
 * scaffold has no binary, so the suite must not pretend one exists.
 */

import assert from 'node:assert/strict';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import { isOpenPPTError } from './errors.js';
import { OpenPPT } from './open-ppt.js';

const addon = process.env.OPEN_PPT_NATIVE;
const skip = addon !== undefined && existsSync(addon) ? false : 'OPEN_PPT_NATIVE does not point at a built addon';

test('capabilities mirror the native matrix instead of a local guess', { skip }, async () => {
    const ppt = new OpenPPT();
    const capabilities = await ppt.capabilities();
    const matrix = capabilities as unknown as {
        project: { import: string; playback: string };
        formats: { pptx: { import: string } };
        checks: { contrast: string; overflow: string };
        playback: { staticSlides: string };
    };

    assert.equal(matrix.project.import, 'supported');
    assert.equal(matrix.project.playback, 'supported');
    assert.equal(matrix.playback.staticSlides, 'supported');
    assert.equal(matrix.checks.contrast, 'supported');
    assert.equal(matrix.checks.overflow, 'unsupported');
    assert.equal(matrix.formats.pptx.import, 'unsupported');
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
