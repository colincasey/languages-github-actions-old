"use strict";

import { readFileSync } from "node:fs"
import { join } from "node:path"
import { parse as urlParse } from "node:url"

import { setFailed } from "@actions/core"
import { exec } from "@actions/exec"
import { find, cacheFile, downloadTool, extractTar } from "@actions/tool-cache"
import { parse as tomlParse } from 'toml'

export function run(rustCommand: string) {
    execute_rust_binary_action(rustCommand).catch(e => {
        if (e instanceof Error) {
            setFailed(e.message)
        }
    })
}

async function execute_rust_binary_action(rustCommand: string) {
    const { platform, env } = process

    if (platform !== 'win32' && platform !== 'darwin' && platform !== 'linux') {
        throw new Error(`Unsupported platform: ${platform}`)
    }

    const toml = tomlParse(readFileSync(join(__dirname, "../Cargo.toml"), 'utf-8'))

    const { name, repository, version } = toml.package

    const githubOrgAndName = urlParse(repository).pathname
        .replace(/^\//, '')
        .replace(/\.git$/, '')

    let cachedPath = find(githubOrgAndName, version)

    if (!cachedPath) {
        const releaseUrl = `https://github.com/${githubOrgAndName}/releases/download/v${version}/${name}-v${version}-${platform}-x64.tar.gz`;
        const downloadPath = await downloadTool(releaseUrl)

        const tempDirectory = env.RUNNER_TEMP
        const extractPath = await extractTar(downloadPath, tempDirectory)

        const binaryName = platform === 'win32' ? `${name}.exe` : name;
        const extractedFile = join(extractPath, binaryName)

        cachedPath = await cacheFile(extractedFile, binaryName, githubOrgAndName, version)
    }

    const rustBinary = join(cachedPath, name);
    await exec(rustBinary, [rustCommand]);
}