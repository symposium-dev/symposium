#!/usr/bin/env node

/**
 * Script to build Docker image with detected Playwright version
 */

const { execSync } = require('child_process');
const { getPlaywrightVersion } = require('./get-playwright-version');

function buildDockerImage() {
    try {
        const version = getPlaywrightVersion();
        console.log(`Building Docker image with Playwright version: ${version}`);

        // Use the detected version or fallback to latest
        // Add 'v' prefix for version numbers, but not for 'latest'
        const dockerVersion = version === 'latest' ? 'latest' : `v${version}`;

        const buildCommand = `docker build --build-arg PLAYWRIGHT_VERSION=${dockerVersion} -t mynah-ui-e2e .`;

        console.log(`Executing: ${buildCommand}`);
        execSync(buildCommand, { stdio: 'inherit' });

        console.log('Docker image built successfully!');
    } catch (error) {
        console.error('Error building Docker image:', error.message);
        process.exit(1);
    }
}

// If called directly, run the build
if (require.main === module) {
    buildDockerImage();
}

module.exports = { buildDockerImage };
