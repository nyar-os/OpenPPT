/**
 * Native integration checks.
 *
 * These only run when `OPEN_PPT_NATIVE` points at a built addon; the default
 * scaffold has no binary, so the suite must not pretend one exists.
 */

import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import { test } from 'node:test';

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
