#!/usr/bin/env node
/**
 * Command-line assembly only.
 *
 * Every command maps options onto one operation of `service.ts`; deck logic,
 * saving and playback stay native. Help text and errors come from
 * `@wae/commander`, so the CLI does not grow its own parser.
 */

import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { createCli } from '@wae/commander';

import * as service from './service.js';
import type { DeckSummary, PlayFrame } from './service.js';
import type { InspectCheck, InspectReport, OpenPPTCapabilities } from './types.js';

const here = path.dirname(fileURLToPath(import.meta.url));

const cli = createCli('open-ppt').locales(path.join(here, '..', 'locales')).intro('cli.intro');

interface ParsedOptions {
    readonly _?: readonly string[];
    readonly [key: string]: unknown;
}

const text = (options: ParsedOptions, key: string): string | undefined =>
    typeof options[key] === 'string' ? (options[key] as string) : undefined;

const list = (options: ParsedOptions, key: string): string[] => {
    const value = options[key];
    if (Array.isArray(value)) {
        return value.filter((item): item is string => typeof item === 'string');
    }
    return typeof value === 'string' ? [value] : [];
};

const flag = (options: ParsedOptions, key: string): boolean => options[key] === true || options[key] === 'true';

const input = (options: ParsedOptions): string | undefined => options._?.[0];

function describeCapabilities(matrix: OpenPPTCapabilities): string {
    const formats = Object.entries(matrix.formats)
        .filter(([, entry]) => entry.import === 'supported')
        .map(([name]) => name);
    const checks = Object.entries(matrix.checks)
        .filter(([, level]) => level === 'supported')
        .map(([name]) => name);
    return [
        `engine: ${matrix.engine.package} ${matrix.engine.version}`,
        `presentation formats: ${formats.length > 0 ? formats.join(', ') : 'none supported'}`,
        `checks: ${checks.length > 0 ? checks.join(', ') : 'none supported'}`,
        `playback: staticSlides=${matrix.playback.staticSlides}`,
    ].join('\n');
}

function describeDeck(deck: DeckSummary): string {
    const lines = [`deck ${deck.id} revision=${deck.revision} slides=${deck.slides.length} size=${deck.size.widthPt}x${deck.size.heightPt}pt`];
    for (const slide of deck.slides) {
        lines.push(`  [${slide.index}] ${slide.id} ${slide.title}${slide.hidden ? ' (hidden)' : ''}`);
    }
    return lines.join('\n');
}

function describeReport(report: InspectReport): string {
    const lines = [`inspect revision=${report.revision} checker=${report.checkerVersion} passed=${report.passed}`];
    for (const issue of report.issues) {
        lines.push(`  ${issue.severity} ${issue.code} ${issue.slideId ?? '-'} ${issue.message}`);
    }
    return lines.join('\n');
}

function describeFrame(frame: PlayFrame, index: number): string {
    const notes = frame.notes === null ? '' : ` notes=${frame.notes}`;
    return `${index}: slide=${frame.slideId ?? '-'}${notes}`;
}

cli.command('capabilities', 'cli.cmd.capabilities').actionCtx(async (_options, _args, ctx) => {
    const matrix = service.capabilities();
    ctx.emitResult(matrix, describeCapabilities(matrix));
    return 0;
});

cli
    .command('create', 'cli.cmd.create')
    .option('--out <file>', 'cli.opt.out')
    .option('--title <title>', 'cli.opt.title')
    .option('--size <ratio>', 'cli.opt.size', { enum: ['16:9', '4:3', '16:10'] })
    .option('--slide <title>...', 'cli.opt.slide')
    .option('--overwrite', 'cli.opt.overwrite')
    .actionCtx(async (options, _args, ctx) => {
        const out = text(options, 'out');
        if (out === undefined) {
            ctx.fail('open-ppt.cli.out-required', { message: '`--out <file>` is required' });
            return 2;
        }
        const created = service.create({ title: text(options, 'title'), size: text(options, 'size'), slides: list(options, 'slide') });
        try {
            const report = service.save(created.handle, out, flag(options, 'overwrite'));
            ctx.emitResult(
                { ...report, deck: created.deck },
                `created ${out} revision=${report.revision} slides=${report.slides}`,
            );
        } finally {
            service.close(created.handle);
        }
        return 0;
    });

cli.command('open', 'cli.cmd.open').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    const opened = service.open(file);
    try {
        ctx.emitResult(opened.deck, describeDeck(opened.deck));
    } finally {
        service.close(opened.handle);
    }
    return 0;
});

cli
    .command('inspect', 'cli.cmd.inspect')
    .option('--checks <list>', 'cli.opt.checks')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const checks = text(options, 'checks')
            ?.split(',')
            .map((name) => name.trim())
            .filter((name) => name.length > 0) as InspectCheck[] | undefined;
        const opened = service.open(file);
        try {
            const report = service.inspect(opened.handle, checks);
            ctx.emitResult(report, describeReport(report));
            return report.passed ? 0 : 1;
        } finally {
            service.close(opened.handle);
        }
    });

cli.command('play', 'cli.cmd.play').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    const opened = service.open(file);
    try {
        const report = service.play(opened.handle);
        ctx.emitResult(report, report.frames.map(describeFrame).join('\n'));
    } finally {
        service.close(opened.handle);
    }
    return 0;
});

const code = await cli.parse(process.argv);
process.exitCode = code;
