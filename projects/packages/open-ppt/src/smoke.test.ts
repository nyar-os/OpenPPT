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
import type { DeckInput } from './index.js';

test('session construction is synchronous and side-effect free', async () => {
    const ppt = new OpenPPT();
    assert.equal(ppt.locale, 'en');
    assert.equal(ppt.disposed, false);
    await ppt.dispose();
    assert.equal(ppt.disposed, true);
});

test('declared capabilities reject with capability/not-implemented', async () => {
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
