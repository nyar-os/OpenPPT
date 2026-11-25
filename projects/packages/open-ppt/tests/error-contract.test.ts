/**
 * Error contract checks against a stub binding.
 *
 * The stub replaces the native display text on purpose: the SDK must branch on
 * the machine-readable `code` and its frozen `params`, never on prose. Unknown
 * codes and payloads that break their shape fall back to one structured
 * `binding/failure`, so every branch stays exhaustive.
 */

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
    isOpenPPTError,
    parseErrorParams,
    setErrorLocale,
    type OpenPPTFailure,
    type OpenPPTError,
} from '../src/errors.js';
import * as service from '../src/service.js';

const dir = mkdtempSync(join(tmpdir(), 'open-ppt-stub-'));
const stubPath = join(dir, 'stub-binding.cjs');
writeFileSync(
    stubPath,
    [
        'function failure() {',
        '    const next = JSON.parse(process.env.OPEN_PPT_STUB || "{}");',
        '    const error = new Error(next.message || "the native text was replaced");',
        '    if (next.code !== undefined) { error.code = next.code; }',
        '    if (next.detail !== undefined) { error.detail = next.detail; }',
        '    return error;',
        '}',
        'module.exports = {',
        '    version() { return "0.0.0-stub"; },',
        '    deck_create() { throw failure(); },',
        '};',
    ].join('\n'),
);
process.env.OPEN_PPT_NATIVE = stubPath;

function capture(): OpenPPTError {
    try {
        service.create({ title: 'stub' });
    } catch (error) {
        assert.ok(isOpenPPTError(error), 'every service failure is an OpenPPTError');
        return error;
    }
    return assert.fail('the stub always throws') as never;
}

function captureWith(payload: Record<string, unknown>): OpenPPTError {
    process.env.OPEN_PPT_STUB = JSON.stringify(payload);
    return capture();
}

/**
 * A consumer branch that depends only on the discriminated union.
 *
 * This is the shape the SDK guarantees: the switch is exhaustive over
 * `failure.code`, and each case reads typed parameters instead of the message.
 */
function branchOf(failure: OpenPPTFailure): string {
    switch (failure.code) {
        case 'object/not-found':
            return `missing:${failure.params.kind}:${failure.params.id}`;
        case 'deck/revision-conflict':
            return `conflict:${failure.params.actual}:${failure.params.expected ?? 'any'}`;
        case 'asset/in-use':
            return `in-use:${failure.params.assetId}`;
        case 'session/argument-invalid':
            return 'argument';
        case 'binding/failure':
            return `fallback:${failure.params.code ?? 'none'}`;
        default:
            return `other:${failure.code}`;
    }
}

test('structured binding failures keep their code and typed params', () => {
    const error = captureWith({
        message: 'REPLACED NATIVE TEXT',
        code: 'object/not-found',
        detail: { params: { kind: 'slide', id: 'slide-x' } },
    });
    assert.equal(error.code, 'object/not-found');
    assert.deepEqual(error.params, { kind: 'slide', id: 'slide-x' });
    assert.equal(error.message, 'the `slide` `slide-x` was not found', 'the product copy replaces the native text');
});

test('replacing the native text never changes the machine branch', () => {
    const first = captureWith({
        message: 'slide slide-x is gone (zh text)',
        code: 'object/not-found',
        detail: { params: { kind: 'slide', id: 'slide-x' } },
    });
    const second = captureWith({
        message: 'NO SUCH SLIDE, HUMAN',
        code: 'object/not-found',
        detail: { params: { kind: 'slide', id: 'slide-x' } },
    });
    assert.equal(branchOf(first.failure), branchOf(second.failure), 'the branch is identical for both texts');
    assert.equal(branchOf(first.failure), 'missing:slide:slide-x');
    assert.equal(first.message, 'the `slide` `slide-x` was not found', 'the copy is rendered from the code, not the text');
    assert.equal(second.message, first.message, 'two native texts produce one product copy');
    assert.deepEqual(first.params, second.params, 'the payload is identical');
});

test('unknown codes fall back to one structured binding failure', () => {
    const error = captureWith({ message: 'native text kept', code: 'weird/code' });
    assert.equal(error.code, 'binding/failure');
    assert.deepEqual(error.params, { reason: 'native text kept', code: 'weird/code' });
    assert.equal(error.message, 'the native binding reported a failure: native text kept', 'the fallback renders the reason');
    assert.equal(branchOf(error.failure), 'fallback:weird/code', 'the raw code stays visible to the fallback branch');
});

test('payloads that break their shape are refused instead of half-forwarded', () => {
    const cases: readonly Record<string, unknown>[] = [
        { params: 'not-an-object' },
        { params: ['x'] },
        { params: {} },
        { params: { kind: 'slide' } },
        { params: { kind: 'slide', id: 'slide-x', reason: 'extra' } },
        { params: { kind: 'slide', id: 7 } },
    ];
    for (const detail of cases) {
        const error = captureWith({ message: 'disk said no', code: 'io/exists', detail });
        assert.equal(error.code, 'binding/failure', `the payload ${JSON.stringify(detail)} is refused`);
        assert.deepEqual(error.params, { reason: 'disk said no', code: 'io/exists' });
    }
});

test('parseErrorParams accepts exactly the frozen shape', () => {
    const accepted = parseErrorParams('deck/revision-conflict', { actual: 3, expected: 2 });
    assert.equal(accepted.ok, true);
    const optional = parseErrorParams('deck/revision-conflict', { actual: 3 });
    assert.equal(optional.ok, true);

    const extra = parseErrorParams('deck/not-found', { revision: 3 });
    assert.deepEqual(extra, { ok: false, rejection: { kind: 'malformed', reason: 'unknown key revision for deck/not-found' } });
    const unknown = parseErrorParams('weird/code', {});
    assert.deepEqual(unknown, { ok: false, rejection: { kind: 'unknown-code', reason: 'unknown code weird/code' } });
});

test('a failure without any code surfaces as binding/failure', () => {
    const error = captureWith({ message: 'boom' });
    assert.equal(error.code, 'binding/failure');
    assert.deepEqual(error.params, { reason: 'boom' });
    assert.equal(error.message, 'the native binding reported a failure: boom');
    assert.equal(error.recoverable, false);
});

test('zh-CN rendering changes only the copy, never the machine fields', () => {
    setErrorLocale('zh-CN');
    try {
        const error = captureWith({
            message: 'REPLACED NATIVE TEXT',
            code: 'object/not-found',
            detail: { params: { kind: 'slide', id: 'slide-x' } },
        });
        assert.equal(error.code, 'object/not-found');
        assert.deepEqual(error.params, { kind: 'slide', id: 'slide-x' });
        assert.equal(error.message, '未找到 `slide` `slide-x`');
    } finally {
        setErrorLocale('en');
    }
});

test('locales interleaved across concurrent translations keep every code stable', async () => {
    const payloads = [
        { code: 'object/not-found', detail: { params: { kind: 'element', id: 'shape-1' } } },
        { code: 'asset/in-use', detail: { params: { assetId: 'asset-9' } } },
        { code: 'session/argument-invalid', detail: { params: { reason: 'row out of range' } } },
        { code: 'deck/revision-conflict', detail: { params: { actual: 4, expected: 3 } } },
    ] as const;
    const captures = await Promise.all(
        payloads.map(async (payload, index) => {
            setErrorLocale(index % 2 === 0 ? 'en' : 'zh-CN');
            await new Promise((resolve) => setTimeout(resolve, 0));
            return captureWith({ message: `text ${index}`, ...payload });
        }),
    );
    setErrorLocale('en');
    captures.forEach((error, index) => {
        const payload = payloads[index];
        assert.equal(error.code, payload.code, `code ${index} is stable regardless of the active locale`);
        assert.deepEqual(error.params, payload.detail.params, `params ${index} are locale-independent`);
    });
    const copies = new Set(captures.map((error) => error.message));
    assert.ok(
        [...copies].every((copy) => copy.length > 0 && !copy.includes('{')),
        'every copy is fully rendered, whatever the active locale was',
    );
});
