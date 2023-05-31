"use strict";

import { readFileSync } from "node:fs"
import { join } from "node:path"
import { parse as urlParse } from "node:url"

import { setFailed, getInput, getBooleanInput, getMultilineInput } from "@actions/core"
import { exec } from "@actions/exec"
import { find, cacheFile, downloadTool, extractTar } from "@actions/tool-cache"
import { parse as tomlParse } from 'toml'

type GetArguments = (inputs: {
    getInput: typeof getInput,
    getBooleanInput: typeof getBooleanInput,
    getMultilineInput: typeof getMultilineInput
}) => string[]

export function invokeWith(getArgs: GetArguments) {
    executeRustBinaryAction(getArgs).catch(e => {
        if (e instanceof Error) {
            setFailed(e.message)
        }
    })
}

async function executeRustBinaryAction(getArgs: GetArguments) {
    const { platform, env } = process

    if (platform !== 'win32' && platform !== 'darwin' && platform !== 'linux') {
        throw new Error(`Unsupported platform: ${platform}`)
    }

    const toml = tomlParse(readFileSync(join(__dirname, "../../Cargo.toml"), 'utf-8'))

    const tempDirectory = env.RUNNER_TEMP
    const { repository, version } = toml.package
    const { name } = toml.bin[0]
    const binaryName = platform === 'win32' ? `${name}.exe` : name;
    const githubOrgAndName = urlParse(repository).pathname
        .replace(/^\//, '')
        .replace(/\.git$/, '')

    // now we should be able to build up our download url which looks something like this:
    // https://github.com/colincasey/languages-github-actions/releases/download/v0.0.0/actions-v0.0.0-darwin-x64.tar.gz
    const releaseUrl = `https://github.com/${githubOrgAndName}/releases/download/v${version}/${name}-v${version}-${platform}-x64.tar.gz`;

    let cachedPath = find(githubOrgAndName, version)
    if (!cachedPath) {
        const downloadPath = await downloadTool(releaseUrl)
        const extractPath = await extractTar(downloadPath, tempDirectory)
        const extractedFile = join(extractPath, binaryName)
        cachedPath = await cacheFile(extractedFile, binaryName, githubOrgAndName, version)
    }

    const rustBinary = join(cachedPath, name);
    const args = getArgs({ getInput, getBooleanInput, getMultilineInput });
    await exec(rustBinary, args);
}