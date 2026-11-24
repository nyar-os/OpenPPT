/**
 * Stable error surface of `@wae/open-ppt`.
 *
 * Codes are contract, not prose: callers compare them, localize around them and
 * never parse the message. A failure therefore keeps its code even when the
 * text is translated.
 */

/** Every stable code the authoring surface may report. */
export const errorCodes = [
    'session/disposed',
    'session/argument-invalid',
    'session/budget-exceeded',
    'deck/not-found',
    'deck/revision-conflict',
    'object/not-found',
    'object/type-mismatch',
    'object/unsupported',
    'format/unsupported',
    'format/invalid-package',
    'format/encrypted',
    'format/macro-refused',
    'asset/missing',
    'asset/too-large',
    'font/missing',
    'render/backend-unavailable',
    'export/loss-exceeded',
    'playback/invalid-state',
    'capability/not-implemented',
    'io/read',
    'io/write',
    'io/exists',
    'io/failure',
    'binding/failure',
] as const;

/** Union of {@link errorCodes}. */
export type OpenPPTErrorCode = (typeof errorCodes)[number];

/** Optional locators a failure can carry so callers can point at the object. */
export interface OpenPPTErrorDetails {
    readonly revision?: number;
    readonly slideId?: string;
    readonly elementId?: string;
    /** Presentation format involved in the failure, without the leading dot. */
    readonly format?: string;
    /** Authoring capability involved in the failure, e.g. `deck.export`. */
    readonly feature?: string;
}

/** Machine-readable per-code parameters delivered by the binding contract. */
export type OpenPPTErrorParams = Readonly<Record<string, unknown>>;

/** Options accepted by {@link OpenPPTError}. */
export interface OpenPPTErrorOptions {
    readonly recoverable?: boolean;
    readonly details?: OpenPPTErrorDetails;
    readonly params?: OpenPPTErrorParams;
    readonly cause?: unknown;
}

/** Every failure thrown or rejected by this package. */
export class OpenPPTError extends Error {
    /** Stable, comparable code. */
    readonly code: OpenPPTErrorCode;
    /** Whether retrying with different input or state can succeed. */
    readonly recoverable: boolean;
    /** Optional locators (`revision`, `slideId`, `elementId`, `format`, `feature`). */
    readonly details: OpenPPTErrorDetails;
    /** Typed parameters of the code, e.g. `{ kind, id }` for `object/not-found`. */
    readonly params: OpenPPTErrorParams;

    constructor(code: OpenPPTErrorCode, message: string, options: OpenPPTErrorOptions = {}) {
        super(message, options.cause === undefined ? undefined : { cause: options.cause });
        this.name = 'OpenPPTError';
        this.code = code;
        this.recoverable = options.recoverable ?? false;
        this.details = Object.freeze({ ...options.details });
        this.params = Object.freeze({ ...options.params });
    }
}

/**
 * Reported by declared methods whose backing capability is not wired yet.
 *
 * The scaffold uses one code for every unimplemented entry point instead of
 * inventing per-method placeholders; each method drops this rejection as it is
 * implemented, and the code disappears with the last one.
 */
export class NotImplementedError extends OpenPPTError {
    constructor(feature: string, message = `\`${feature}\` is not implemented yet`) {
        super('capability/not-implemented', message, { recoverable: false, details: { feature } });
        this.name = 'NotImplementedError';
    }
}

/** Narrows an unknown thrown value to {@link OpenPPTError}. */
export function isOpenPPTError(value: unknown): value is OpenPPTError {
    return value instanceof OpenPPTError;
}

/**
 * Product copy for each stable code, rendered from its typed `params`.
 *
 * The binding's own text is only a fallback for codes this catalog does not
 * carry yet; the machine-readable `code` and `params` never depend on locale.
 */
const messageCatalog: Readonly<Record<string, Readonly<Record<string, string>>>> = {
    en: {
        'session/disposed': 'the OpenPPT session has been disposed; create a new instance',
        'session/argument-invalid': 'the request was rejected: {reason}',
        'session/budget-exceeded': 'the operation exceeded the session budget',
        'deck/not-found': 'the deck handle is not open',
        'deck/revision-conflict': 'the deck changed at revision {actual}; the edit expected {expected}',
        'object/not-found': 'the `{kind}` `{id}` was not found',
        'object/type-mismatch': 'the `{kind}` `{id}` does not accept this operation',
        'object/unsupported': 'the addressed object does not support this operation',
        'format/unsupported': '`{format}` is not an offered format',
        'format/invalid-package': 'the package failed validation: {reason}',
        'format/encrypted': 'encrypted packages cannot be opened',
        'format/macro-refused': 'macro-bearing packages are refused',
        'asset/missing': 'the referenced asset is missing from the package',
        'asset/too-large': 'the asset exceeds the size budget',
        'font/missing': 'the required font `{font}` is not available',
        'render/backend-unavailable': 'no render backend is available on this platform',
        'export/loss-exceeded': 'the export dropped {count} constructs and the loss policy is `{policy}`',
        'playback/invalid-state': 'playback cannot run in the current state',
        'capability/not-implemented': 'the `{feature}` capability is not implemented yet',
        'io/read': 'could not read `{path}` ({kind})',
        'io/write': 'could not publish `{path}`',
        'io/exists': '`{path}` already exists; pass overwrite to replace it',
        'io/failure': 'the file operation failed ({kind})',
        'binding/failure': 'the native binding reported a failure without a stable code',
    },
    'zh-CN': {
        'session/disposed': 'OpenPPT 会话已释放；请创建新实例',
        'session/argument-invalid': '请求被拒绝：{reason}',
        'session/budget-exceeded': '操作超出会话预算',
        'deck/not-found': '该幻灯片组句柄未打开',
        'deck/revision-conflict': '幻灯片组已变更到修订 {actual}；本次编辑期望 {expected}',
        'object/not-found': '未找到 `{kind}` `{id}`',
        'object/type-mismatch': '`{kind}` `{id}` 不接受此操作',
        'object/unsupported': '目标对象不支持此操作',
        'format/unsupported': '`{format}` 不是可提供的格式',
        'format/invalid-package': '包校验失败：{reason}',
        'format/encrypted': '无法打开加密包',
        'format/macro-refused': '拒绝包含宏的包',
        'asset/missing': '引用的素材在包中缺失',
        'asset/too-large': '素材超出体积预算',
        'font/missing': '所需字体 `{font}` 不可用',
        'render/backend-unavailable': '当前平台没有可用的渲染后端',
        'export/loss-exceeded': '导出丢弃了 {count} 个构造，损失策略为 `{policy}`',
        'playback/invalid-state': '当前状态无法播放',
        'capability/not-implemented': '`{feature}` 能力尚未实现',
        'io/read': '无法读取 `{path}`（{kind}）',
        'io/write': '无法发布 `{path}`',
        'io/exists': '`{path}` 已存在；传入 overwrite 以替换',
        'io/failure': '文件操作失败（{kind}）',
        'binding/failure': '原生绑定返回了没有稳定码的失败',
    },
};

let currentLocale = 'en';

/** Selects the locale used to render error copy; `zh*` maps to `zh-CN`. */
export function setErrorLocale(tag: string): void {
    currentLocale = tag.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en';
}

/**
 * Renders the product copy for a code and its params.
 *
 * Unknown codes and missing translations fall back to the binding's own text,
 * so a replaced or removed native message never changes which branch a caller
 * takes: that branch is decided by `code` and `params`.
 */
export function renderErrorCopy(code: string, params: OpenPPTErrorParams, fallback: string): string {
    const template = messageCatalog[currentLocale]?.[code];
    if (template === undefined) {
        return fallback;
    }
    let complete = true;
    const rendered = template.replace(/\{(\w+)\}/g, (match, key: string) => {
        if (params[key] === undefined) {
            complete = false;
            return match;
        }
        return String(params[key]);
    });
    return complete ? rendered : fallback;
}
