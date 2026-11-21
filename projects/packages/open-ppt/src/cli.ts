#!/usr/bin/env node
/**
 * Command-line assembly only.
 *
 * Every command maps options onto one operation of `service.ts`; deck logic,
 * saving and playback stay native. Help text and errors come from
 * `@wae/commander`, so the CLI does not grow its own parser.
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { createCli } from '@wae/commander';

import * as service from './service.js';
import { runGui } from './gui.js';
import type { DeckSummary, PlayFrame, PptxPresentation } from './service.js';
import type { ExportFormat, InspectCheck, InspectReport, OpenPPTCapabilities } from './types.js';

const here = path.dirname(fileURLToPath(import.meta.url));

const cli = createCli('open-ppt').locales(path.join(here, '..', 'locales')).intro('cli.intro');

// Exported for contract audits that import the built entry to inspect the
// command tree. Nothing runs until the file is executed as the program entry.
export { cli };

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

/** Extensions the presentation reader plays without a native project. */
const presentationExtensions = new Set(['pptx', 'pptm', 'ppsx', 'ppsm', 'potx', 'potm']);

function isPresentationPath(file: string): boolean {
    return presentationExtensions.has(path.extname(file).slice(1).toLowerCase());
}

function describePresentation(presentation: PptxPresentation): string {
    const title = presentation.title ?? path.basename(presentation.label);
    const width = Math.round(presentation.size.widthPx);
    const height = Math.round(presentation.size.heightPx);
    const lines = [`presentation ${title} slides=${presentation.slides.length} size=${width}x${height}px`];
    for (const slide of presentation.slides) {
        lines.push(`  [${slide.index}] ${Math.round(slide.widthPx)}x${Math.round(slide.heightPx)}px`);
    }
    for (const issue of presentation.issues) {
        const location = issue.slide === null ? '' : ` slide=${issue.slide}`;
        lines.push(`  ${issue.status} ${issue.code}${location} ${issue.message}`);
    }
    return lines.join('\n');
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
    .command('import', 'cli.cmd.import')
    .option('--out <file>', 'cli.opt.importOut')
    .option('--overwrite', 'cli.opt.overwrite')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a presentation file is required' });
            return 2;
        }
        const imported = service.importPptx(file);
        try {
            const out = text(options, 'out');
            const saved = out === undefined ? undefined : service.save(imported.handle, out, flag(options, 'overwrite'));
            const lines = [describeDeck(imported.deck)];
            if (saved !== undefined) {
                lines.push(`saved ${saved.path} bytes=${saved.bytes}`);
            }
            for (const loss of imported.losses) {
                lines.push(`  ${loss.status} ${loss.code} slide=${loss.slide ?? '-'} shape=${loss.shape ?? '-'} ${loss.message}`);
            }
            ctx.emitResult(
                { deck: imported.deck, losses: imported.losses, ...(saved === undefined ? {} : { saved }) },
                lines.join('\n'),
            );
        } finally {
            service.close(imported.handle);
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

cli
    .command('play', 'cli.cmd.play')
    .option('--out <dir>', 'cli.opt.svg')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project or presentation file is required' });
            return 2;
        }
        const out = text(options, 'out');
        if (isPresentationPath(file)) {
            const opened = service.openPptx(file);
            try {
                if (out !== undefined) {
                    mkdirSync(out, { recursive: true });
                    for (const slide of opened.presentation.slides) {
                        const frame = service.pptxSlide(opened.handle, slide.index);
                        writeFileSync(path.join(out, `slide-${slide.index}.svg`), frame.svg, 'utf8');
                    }
                }
                ctx.emitResult(opened.presentation, describePresentation(opened.presentation));
            } finally {
                service.closePptx(opened.handle);
            }
            return 0;
        }
        if (out !== undefined) {
            ctx.fail('open-ppt.cli.out-unsupported', {
                message: '`--out` writes SVG frames only for PresentationML files, not for native projects',
            });
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

cli
    .command('render', 'cli.cmd.render')
    .option('--out <dir>', 'cli.opt.outDir')
    .option('--format <format>', 'cli.opt.renderFormat', { enum: ['svg'] })
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const format = text(options, 'format') ?? 'svg';
        if (format !== 'svg') {
            ctx.fail('open-ppt.cli.format-unsupported', { message: `the \`${format}\` renderer is not implemented` });
            return 2;
        }
        const out = text(options, 'out');
        const opened = service.open(file);
        try {
            const report = service.render(opened.handle, { format: 'svg' });
            let pages = report.pages;
            if (out !== undefined) {
                mkdirSync(out, { recursive: true });
                pages = pages.map((page) => {
                    const target = path.join(out, `slide-${page.index + 1}.svg`);
                    writeFileSync(target, page.svg, 'utf8');
                    return { ...page, path: target, bytes: Buffer.byteLength(page.svg) };
                });
            }
            const lines = [`render revision=${report.revision} format=svg pages=${pages.length}`];
            for (const page of pages) {
                lines.push(`  [${page.index}] ${page.widthPx}x${page.heightPx}px ${page.path ?? page.slideId}`);
            }
            for (const loss of report.losses) {
                lines.push(`  ${loss.code} ${loss.slideId ?? '-'} ${loss.message}`);
            }
            ctx.emitResult({ ...report, pages }, lines.join('\n'));
        } finally {
            service.close(opened.handle);
        }
        return 0;
    });

cli
    .command('export', 'cli.cmd.export')
    .option('--format <format>', 'cli.opt.exportFormat', { enum: ['pptx', 'ppsx', 'potx'] })
    .option('--out <file>', 'cli.opt.exportOut')
    .option('--overwrite', 'cli.opt.overwrite')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const target = text(options, 'out');
        if (target === undefined) {
            ctx.fail('open-ppt.cli.out-required', { message: '`--out <file>` is required' });
            return 2;
        }
        const requested = text(options, 'format') ?? 'pptx';
        if (requested !== 'pptx' && requested !== 'ppsx' && requested !== 'potx') {
            ctx.fail('open-ppt.cli.format-unsupported', { message: `\`${requested}\` is not a supported export target` });
            return 2;
        }
        const format: ExportFormat = requested === 'ppsx' ? 'ppsx' : requested === 'potx' ? 'potx' : 'pptx';
        const opened = service.open(file);
        try {
            const report = service.exportDeck(opened.handle, {
                format,
                target,
                overwrite: flag(options, 'overwrite'),
            });
            const lines = [`export ${target} revision=${report.revision} format=${report.format} bytes=${report.bytes}`];
            for (const loss of report.losses) {
                lines.push(`  ${loss.code} ${loss.slideId ?? '-'} ${loss.message}`);
            }
            ctx.emitResult(report, lines.join('\n'));
        } finally {
            service.close(opened.handle);
        }
        return 0;
    });

cli.command('gui', 'cli.cmd.gui').actionCtx(async (_options, _args, ctx) => {
    try {
        const status = await runGui({});
        ctx.emitResult(status, `window ${status.state}`);
        return 0;
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        ctx.fail('open-ppt.cli.gui-failed', { message });
        return 1;
    }
});

const entry = process.argv[1];
if (entry !== undefined && import.meta.url === pathToFileURL(entry).href) {
    const code = await cli.parse(process.argv);
    process.exitCode = code;
}
