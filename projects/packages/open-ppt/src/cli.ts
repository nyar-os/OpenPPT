#!/usr/bin/env node
/**
 * Command-line assembly only.
 *
 * Every command maps options onto one method of `new OpenPPT()` or of the
 * `Deck` it returns; deck logic, saving and playback stay native. Help text and
 * errors come from `@wae/commander`, so the CLI does not grow its own parser.
 */

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { createCli } from '@wae/commander';
import type { CommandContext } from '@wae/commander';

import { OpenPPT } from './open-ppt.js';
import { runGui } from './gui.js';
import type {
    DeckOutline,
    DeckSummary,
    EditOperation,
    EditReport,
    PlayFrame,
    PptxPresentation,
    SearchHitList,
    SlideDetail,
} from './index.js';
import type {
    AssetInfo,
    DeckSize,
    ExportFormat,
    InspectCheck,
    InspectReport,
    OpenPPTCapabilities,
    SaveReport,
} from './types.js';

const here = path.dirname(fileURLToPath(import.meta.url));

const cli = createCli('open-ppt').locales(path.join(here, '..', 'locales')).intro('cli.intro');

/** Runs `work` against one session and always releases its handles. */
async function withSession<T>(work: (ppt: OpenPPT) => Promise<T>): Promise<T> {
    const ppt = new OpenPPT();
    try {
        return await work(ppt);
    } finally {
        await ppt.dispose();
    }
}

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

function describeSlide(detail: SlideDetail): string {
    const lines = [
        `slide ${detail.id} index=${detail.index} title=${detail.title}${detail.hidden ? ' (hidden)' : ''} layout=${detail.layout ?? '-'} notes=${detail.notes ?? '-'}`,
    ];
    for (const element of detail.elements) {
        const transform = element.transform;
        const rotation = transform.rotationDeg === null ? '' : ` rotation=${transform.rotationDeg}deg`;
        const text = element.text === undefined ? '' : ` text=${JSON.stringify(element.text)}`;
        lines.push(
            `  ${element.id} ${element.kind} ${transform.xPt},${transform.yPt} ${transform.widthPt}x${transform.heightPt}pt${rotation} locked=${element.locked} alt=${element.alt ?? '-'}${text}`,
        );
    }
    return lines.join('\n');
}

function describeSearch(revision: number, query: string, hits: SearchHitList): string {
    const lines = [
        `search revision=${revision} query=${JSON.stringify(query)} matches=${hits.length}${hits.truncated ? ' truncated=true' : ''}`,
    ];
    for (const hit of hits) {
        const element = hit.elementId === undefined ? '' : ` ${hit.elementId}`;
        lines.push(`  [${hit.slideIndex}] ${hit.slideId}${element} ${hit.kind} @${hit.start}+${hit.length} ${singleLine(hit.text)}`);
    }
    return lines.join('\n');
}

function describeOutline(outline: DeckOutline): string {
    const lines = [`outline revision=${outline.revision} slides=${outline.slides.length}`];
    for (const slide of outline.slides) {
        const notes = slide.notes === undefined ? '' : ` notes=${JSON.stringify(slide.notes)}`;
        const points = slide.points === undefined ? '' : ` points=${slide.points.length}`;
        lines.push(
            `  [${slide.index}] ${slide.id} ${slide.title}${slide.hidden ? ' (hidden)' : ''} elements=${slide.elementCount}${notes}${points}`,
        );
        for (const point of slide.points ?? []) {
            lines.push(`      - ${point}`);
        }
    }
    return lines.join('\n');
}

function singleLine(text: string): string {
    return text.replace(/\s+/g, ' ').trim();
}

function describeAssets(assets: ReadonlyArray<AssetInfo>): string {
    const lines = [`assets total=${assets.length}`];
    for (const asset of assets) {
        const license = asset.license === undefined ? '' : ` license=${asset.license}`;
        lines.push(`  ${asset.id} ${asset.name} ${asset.mediaType} bytes=${asset.bytes} sha256=${asset.sha256}${license}`);
    }
    return lines.join('\n');
}

function describeEdit(report: EditReport, saved: SaveReport | undefined): string {
    const lines = [`edited revision=${report.revision} results=${report.results.length}`];
    report.results.forEach((result, index) => {
        lines.push(`  [${index}] ${result ?? '-'}`);
    });
    for (const [token, id] of Object.entries(report.resolved)) {
        lines.push(`  resolved ${token} -> ${id}`);
    }
    if (saved !== undefined) {
        lines.push(`saved ${saved.target} revision=${saved.revision} bytes=${saved.bytes}`);
    }
    return lines.join('\n');
}

function editFailure(ctx: CommandContext, message: string): never {
    return ctx.fail('open-ppt.cli.edit-invalid', { message });
}

/** Reads an operations array, or a full edit request that carries one. */
function readEditScript(raw: string, ctx: CommandContext): { operations: ReadonlyArray<EditOperation>; expectedRevision?: number } {
    const parsed = (() => {
        try {
            return JSON.parse(raw) as unknown;
        } catch (error) {
            return editFailure(ctx, `the edit script is not valid JSON: ${error instanceof Error ? error.message : String(error)}`);
        }
    })();
    let operations: unknown[] | undefined;
    let expectedRevision: unknown;
    if (Array.isArray(parsed)) {
        operations = parsed;
    } else if (parsed !== null && typeof parsed === 'object') {
        const request = parsed as { readonly operations?: unknown; readonly expectedRevision?: unknown };
        if (Array.isArray(request.operations)) {
            operations = request.operations;
            expectedRevision = request.expectedRevision;
        }
    }
    if (operations === undefined) {
        editFailure(ctx, 'an edit script is an operations array or an object with an `operations` array');
    }
    if (operations.length === 0) {
        editFailure(ctx, 'an edit script needs at least one operation');
    }
    for (const [index, operation] of operations.entries()) {
        if (operation === null || typeof operation !== 'object' || Array.isArray(operation)) {
            editFailure(ctx, `operation ${index} must be a JSON object`);
        }
    }
    if (expectedRevision === undefined) {
        return { operations: operations as ReadonlyArray<EditOperation> };
    }
    if (typeof expectedRevision !== 'number' || !Number.isInteger(expectedRevision) || expectedRevision < 0) {
        editFailure(ctx, '`expectedRevision` must be a non-negative integer');
    }
    return { operations: operations as ReadonlyArray<EditOperation>, expectedRevision };
}

function readRevision(raw: string | undefined, ctx: CommandContext): number | undefined {
    if (raw === undefined) {
        return undefined;
    }
    const value = Number(raw);
    if (raw.trim().length === 0 || !Number.isInteger(value) || value < 0) {
        return ctx.fail('open-ppt.cli.edit-invalid', { message: '`--expected-revision <n>` must be a non-negative integer' });
    }
    return value;
}

cli.command('capabilities', 'cli.cmd.capabilities').actionCtx(async (_options, _args, ctx) => {
    const matrix = await withSession((ppt) => ppt.capabilities());
    ctx.emitResult(matrix, describeCapabilities(matrix));
    return 0;
});

cli
    .command('create', 'cli.cmd.create')
    .option('--out <file>', 'cli.opt.out')
    .option('--title <title>', 'cli.opt.title')
    .option('--size <ratio>', 'cli.opt.size', { enum: ['16:9', '4:3', '16:10'] })
    .option('--theme <name>', 'cli.opt.theme')
    .option('--slide <title>...', 'cli.opt.slide')
    .option('--overwrite', 'cli.opt.overwrite')
    .actionCtx(async (options, _args, ctx) => {
        const out = text(options, 'out');
        if (out === undefined) {
            ctx.fail('open-ppt.cli.out-required', { message: '`--out <file>` is required' });
            return 2;
        }
        await withSession(async (ppt) => {
            const deck = await ppt.create({
                title: text(options, 'title'),
                size: text(options, 'size') as DeckSize | undefined,
                theme: text(options, 'theme'),
                slides: list(options, 'slide'),
            });
            const report = await deck.save({ target: out, overwrite: flag(options, 'overwrite') });
            ctx.emitResult(
                { ...report, deck: deck.summary() },
                `created ${out} revision=${report.revision} slides=${deck.summary().slides.length}`,
            );
        });
        return 0;
    });

cli.command('open', 'cli.cmd.open').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    await withSession(async (ppt) => {
        const deck = await ppt.open({ path: file });
        const summary = deck.summary();
        ctx.emitResult(summary, describeDeck(summary));
    });
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
        await withSession(async (ppt) => {
            const deck = await ppt.importPptx(file);
            const out = text(options, 'out');
            const saved = out === undefined ? undefined : await deck.save({ target: out, overwrite: flag(options, 'overwrite') });
            const summary = deck.summary();
            const lines = [describeDeck(summary)];
            if (saved !== undefined) {
                lines.push(`saved ${saved.target} bytes=${saved.bytes}`);
            }
            for (const loss of deck.importLosses) {
                lines.push(`  ${loss.status} ${loss.code} slide=${loss.slide ?? '-'} shape=${loss.shape ?? '-'} ${loss.message}`);
            }
            ctx.emitResult(
                { deck: summary, losses: deck.importLosses, ...(saved === undefined ? {} : { saved }) },
                lines.join('\n'),
            );
        });
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
        let passed = false;
        await withSession(async (ppt) => {
            const report = await ppt.inspect({ path: file }, { checks });
            ctx.emitResult(report, describeReport(report));
            passed = report.passed;
        });
        return passed ? 0 : 1;
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
            await withSession(async (ppt) => {
                const reader = await ppt.readPptx(file);
                if (out !== undefined) {
                    mkdirSync(out, { recursive: true });
                    for (const slide of reader.presentation.slides) {
                        const frame = reader.slide(slide.index);
                        writeFileSync(path.join(out, `slide-${slide.index}.svg`), frame.svg, 'utf8');
                    }
                }
                ctx.emitResult(reader.presentation, describePresentation(reader.presentation));
            });
            return 0;
        }
        if (out !== undefined) {
            ctx.fail('open-ppt.cli.out-unsupported', {
                message: '`--out` writes SVG frames only for PresentationML files, not for native projects',
            });
            return 2;
        }
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const report = await deck.play();
            ctx.emitResult(report, report.frames.map(describeFrame).join('\n'));
        });
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
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const report = await deck.render({ format: 'svg', target: out });
            const lines = [`render revision=${report.revision} format=svg pages=${report.pages.length}`];
            for (const page of report.pages) {
                lines.push(`  [${page.index}] ${page.widthPx}x${page.heightPx}px ${page.path ?? page.slideId}`);
            }
            for (const loss of report.losses) {
                lines.push(`  ${loss.code} ${loss.slideId ?? '-'} ${loss.message}`);
            }
            ctx.emitResult(report, lines.join('\n'));
        });
        return 0;
    });

cli
    .command('contact-sheet', 'cli.cmd.contactSheet')
    .option('--out <file>', 'cli.opt.contactSheetOut')
    .option('--columns <n>', 'cli.opt.contactSheetColumns')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const out = text(options, 'out');
        if (out === undefined) {
            ctx.fail('cli.error.contactSheetOutMissing');
        }
        const rawColumns = text(options, 'columns');
        let columns: number | undefined;
        if (rawColumns !== undefined) {
            const value = Number(rawColumns);
            if (rawColumns.trim().length === 0 || !Number.isInteger(value) || value < 1) {
                ctx.fail('cli.error.contactSheetColumnsInvalid');
            }
            columns = value;
        }
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const sheet = await deck.contactSheet({ target: out, columns });
            const lines = [
                `contact-sheet ${out} slides=${sheet.slideIds.length} columns=${sheet.columns} size=${sheet.widthPx}x${sheet.heightPx}px bytes=${sheet.bytes}`,
            ];
            ctx.emitResult(sheet, lines.join('\n'));
        });
        return 0;
    });

cli
    .command('export', 'cli.cmd.export')
    .option('--format <format>', 'cli.opt.exportFormat', { enum: ['pptx', 'ppsx', 'potx', 'pdf'] })
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
        if (requested !== 'pptx' && requested !== 'ppsx' && requested !== 'potx' && requested !== 'pdf') {
            ctx.fail('open-ppt.cli.format-unsupported', { message: `\`${requested}\` is not a supported export target` });
            return 2;
        }
        const format: ExportFormat =
            requested === 'ppsx' ? 'ppsx' : requested === 'potx' ? 'potx' : requested === 'pdf' ? 'pdf' : 'pptx';
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const report = await deck.export({
                format,
                target,
                overwrite: flag(options, 'overwrite'),
            });
            const lines = [`export ${target} revision=${report.revision} format=${report.format} bytes=${report.bytes}`];
            for (const loss of report.losses) {
                lines.push(`  ${loss.code} ${loss.slideId ?? '-'} ${loss.message}`);
            }
            ctx.emitResult(report, lines.join('\n'));
        });
        return 0;
    });

function readScriptFile(file: string, ctx: CommandContext): string {
    try {
        return readFileSync(file, 'utf8');
    } catch (error) {
        return ctx.fail('open-ppt.cli.script-unreadable', {
            message: `cannot read edit script \`${file}\`: ${error instanceof Error ? error.message : String(error)}`,
        });
    }
}

cli
    .command('edit', 'cli.cmd.edit')
    .option('--ops <json>', 'cli.opt.ops')
    .option('--script <file>', 'cli.opt.editScript')
    .option('--out <file>', 'cli.opt.out')
    .option('--overwrite', 'cli.opt.overwrite')
    .option('--expected-revision <n>', 'cli.opt.expectedRevision')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const ops = text(options, 'ops');
        const script = text(options, 'script');
        let raw: string;
        if (ops !== undefined && script !== undefined) {
            ctx.fail('open-ppt.cli.edit-source', { message: '`--ops <json>` and `--script <file>` are mutually exclusive' });
            return 2;
        } else if (script !== undefined) {
            raw = readScriptFile(script, ctx);
        } else if (ops !== undefined) {
            raw = ops;
        } else {
            ctx.fail('open-ppt.cli.edit-source', { message: 'exactly one of `--ops <json>` or `--script <file>` is required' });
            return 2;
        }
        const request = readEditScript(raw, ctx);
        const flagRevision = readRevision(text(options, 'expected-revision'), ctx);
        const expectedRevision = flagRevision ?? request.expectedRevision;
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const report = await deck.edit(request.operations, { expectedRevision });
            const out = text(options, 'out');
            const saved = out === undefined ? undefined : await deck.save({ target: out, overwrite: flag(options, 'overwrite') });
            ctx.emitResult(
                { revision: report.revision, results: report.results, resolved: report.resolved, ...(saved === undefined ? {} : { saved }) },
                describeEdit(report, saved),
            );
        });
        return 0;
    });

cli.command('slide', 'cli.cmd.slide').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    const slideId = options._?.[1];
    if (slideId === undefined) {
        ctx.fail('open-ppt.cli.slide-required', { message: 'a slide id is required' });
        return 2;
    }
    await withSession(async (ppt) => {
        const deck = await ppt.open({ path: file });
        const detail = await deck.slide(slideId);
        ctx.emitResult(detail, describeSlide(detail));
    });
    return 0;
});

cli
    .command('search', 'cli.cmd.search')
    .option('--case-sensitive', 'cli.opt.caseSensitive')
    .option('--regex', 'cli.opt.regex')
    .option('--notes', 'cli.opt.notes')
    .option('--limit <n>', 'cli.opt.limit')
    .actionCtx(async (options, _args, ctx) => {
        const file = input(options);
        if (file === undefined) {
            ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
            return 2;
        }
        const query = options._?.[1];
        if (query === undefined || query.trim().length === 0) {
            ctx.fail('cli.error.searchQueryMissing');
        }
        const rawLimit = text(options, 'limit');
        let limit: number | undefined;
        if (rawLimit !== undefined) {
            const value = Number(rawLimit);
            if (rawLimit.trim().length === 0 || !Number.isInteger(value) || value < 1) {
                ctx.fail('cli.error.searchLimitInvalid');
            }
            limit = value;
        }
        await withSession(async (ppt) => {
            const deck = await ppt.open({ path: file });
            const hits = await deck.search(query, {
                caseSensitive: flag(options, 'case-sensitive'),
                regex: flag(options, 'regex'),
                notes: flag(options, 'notes'),
                ...(limit === undefined ? {} : { limit }),
            });
            ctx.emitResult(
                { revision: deck.revision, query, hits, truncated: hits.truncated },
                describeSearch(deck.revision, query, hits),
            );
        });
        return 0;
    });

cli.command('outline', 'cli.cmd.outline').option('--points', 'cli.opt.outlinePoints').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    await withSession(async (ppt) => {
        const deck = await ppt.open({ path: file });
        const outline = deck.outline({ points: flag(options, 'points') });
        ctx.emitResult(outline, describeOutline(outline));
    });
    return 0;
});

cli.command('assets', 'cli.cmd.assets').actionCtx(async (options, _args, ctx) => {
    const file = input(options);
    if (file === undefined) {
        ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
        return 2;
    }
    await withSession(async (ppt) => {
        const deck = await ppt.open({ path: file });
        const assets = await deck.assets.list();
        ctx.emitResult(assets, describeAssets(assets));
    });
    return 0;
});

function historyCommand(name: 'undo' | 'redo'): void {
    cli
        .command(name, `cli.cmd.${name}`)
        .option('--out <file>', 'cli.opt.out')
        .option('--overwrite', 'cli.opt.overwrite')
        .actionCtx(async (options, _args, ctx) => {
            const file = input(options);
            if (file === undefined) {
                ctx.fail('open-ppt.cli.file-required', { message: 'a project file is required' });
                return 2;
            }
            await withSession(async (ppt) => {
                const deck = await ppt.open({ path: file });
                const changed = name === 'undo' ? await deck.undo() : await deck.redo();
                const revision = deck.revision;
                const out = text(options, 'out');
                const saved = out === undefined ? undefined : await deck.save({ target: out, overwrite: flag(options, 'overwrite') });
                const lines = [`${name} revision=${revision} changed=${changed}`];
                if (saved !== undefined) {
                    lines.push(`saved ${saved.target} revision=${saved.revision} bytes=${saved.bytes}`);
                }
                ctx.emitResult(
                    { revision, changed, ...(saved === undefined ? {} : { saved }) },
                    lines.join('\n'),
                );
            });
            return 0;
        });
}

historyCommand('undo');
historyCommand('redo');

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
