/**
 * Build and release configuration for OpenPPT.
 *
 * Pure literals only: `wae` parses this file statically and never executes it.
 * Build order is fixed: native → ts → ui → skills → staging.
 *
 * The packages live under `projects/packages`; the native addon is built from
 * the `open-ppt-napi` crate and staged into the per-platform packages.
 */
import { defineConfig } from '@wae/wae';

export default defineConfig({
    native: {
        crate: 'open-ppt-napi',
        cdylib: 'open_ppt_napi',
        targets: [
            {
                platform: 'win32',
                arch: 'x64',
                triple: 'x86_64-pc-windows-msvc',
                package: '@wae/open-ppt-win32-x64',
                nodeFile: 'open-ppt.win32-x64.node',
                dir: 'projects/packages/open-ppt-win32-x64',
            },
            {
                platform: 'darwin',
                arch: 'x64',
                triple: 'x86_64-apple-darwin',
                package: '@wae/open-ppt-darwin-x64',
                nodeFile: 'open-ppt.darwin-x64.node',
                dir: 'projects/packages/open-ppt-darwin-x64',
            },
            {
                platform: 'darwin',
                arch: 'arm64',
                triple: 'aarch64-apple-darwin',
                package: '@wae/open-ppt-darwin-arm64',
                nodeFile: 'open-ppt.darwin-arm64.node',
                dir: 'projects/packages/open-ppt-darwin-arm64',
            },
            {
                platform: 'linux',
                arch: 'x64',
                triple: 'x86_64-unknown-linux-gnu',
                package: '@wae/open-ppt-linux-x64',
                nodeFile: 'open-ppt.linux-x64.node',
                dir: 'projects/packages/open-ppt-linux-x64',
            },
            {
                platform: 'linux',
                arch: 'arm64',
                triple: 'aarch64-unknown-linux-gnu',
                package: '@wae/open-ppt-linux-arm64',
                nodeFile: 'open-ppt.linux-arm64.node',
                dir: 'projects/packages/open-ppt-linux-arm64',
            },
        ],
    },
    packages: [
        {
            name: '@wae/open-ppt-win32-x64',
            dir: 'projects/packages/open-ppt-win32-x64',
            os: ['win32'],
            cpu: ['x64'],
            main: 'open-ppt.win32-x64.node',
        },
        {
            name: '@wae/open-ppt-darwin-x64',
            dir: 'projects/packages/open-ppt-darwin-x64',
            os: ['darwin'],
            cpu: ['x64'],
            main: 'open-ppt.darwin-x64.node',
        },
        {
            name: '@wae/open-ppt-darwin-arm64',
            dir: 'projects/packages/open-ppt-darwin-arm64',
            os: ['darwin'],
            cpu: ['arm64'],
            main: 'open-ppt.darwin-arm64.node',
        },
        {
            name: '@wae/open-ppt-linux-x64',
            dir: 'projects/packages/open-ppt-linux-x64',
            os: ['linux'],
            cpu: ['x64'],
            main: 'open-ppt.linux-x64.node',
        },
        {
            name: '@wae/open-ppt-linux-arm64',
            dir: 'projects/packages/open-ppt-linux-arm64',
            os: ['linux'],
            cpu: ['arm64'],
            main: 'open-ppt.linux-arm64.node',
        },
    ],
    build: {
        ts: { packages: ['@wae/open-ppt'] },
        ui: { package: '@wae/open-ppt-ui' },
        stage: [
            {
                from: 'projects/packages/open-ppt-ui/dist/static',
                to: 'projects/packages/open-ppt/dist/gui',
                clean: true,
            },
        ],
        outputs: [
            'projects/packages/open-ppt/dist/cli.js',
            'projects/packages/open-ppt/dist/gui/index.html',
        ],
    },
    skills: {
        packages: [
            {
                dir: 'projects/packages/open-ppt-skills/skills',
                packageJson: 'projects/packages/open-ppt-skills/package.json',
                appPackageJson: 'projects/packages/open-ppt/package.json',
                cliEntry: 'projects/packages/open-ppt/dist/cli.js',
                commandName: 'open-ppt',
            },
        ],
    },
    // `release` is intentionally unset: nifty publishes the app package and
    // the platform packages; the studio and skills packages stay private.
    ci: {
        repo: 'nyar-os/OpenPPT',
        workflows: ['check-rust.yml', 'check-typescript.yml'],
        requiredJobs: ['build', 'check'],
        shaBinding: false,
    },
    install: { shortcutDefault: true },
    audit: { actor: { kind: 'flag', id: 'flag', source: 'flag' } },
    window: { mode: 'single', secondLaunch: 'focus' },
});
