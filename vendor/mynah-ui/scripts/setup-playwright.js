#!/usr/bin/env node

/**
 * Script to setup Playwright with version-agnostic approach
 * This ensures consistent versions across local and Docker environments
 */

const { execSync } = require('child_process');
const path = require('path');
const fs = require('fs');
const { getPlaywrightVersion } = require('./get-playwright-version');

function setupPlaywright(targetDir = null) {
    try {
        const version = getPlaywrightVersion();
        console.log(`Setting up Playwright version: ${version}`);

        // Determine target directory
        const uiTestsPath = targetDir || path.join(__dirname, '../ui-tests');

        // Ensure target directory exists
        if (!fs.existsSync(uiTestsPath)) {
            throw new Error(`Target directory does not exist: ${uiTestsPath}`);
        }

        const installCommand =
            version === 'latest'
                ? 'npm install @playwright/test@latest playwright@latest --save-dev'
                : `npm install @playwright/test@${version} playwright@${version} --save-dev`;

        console.log(`Installing Playwright in ${uiTestsPath}...`);
        execSync(installCommand, {
            stdio: 'inherit',
            cwd: uiTestsPath,
        });

        // Install Playwright browsers with dependencies
        console.log('Installing Playwright browsers with dependencies...');
        execSync('npx playwright install --with-deps', {
            stdio: 'inherit',
            cwd: uiTestsPath,
        });

        console.log('Playwright setup completed successfully!');
        return true;
    } catch (error) {
        console.error('Error setting up Playwright:', error.message);
        throw error; // Re-throw to allow caller to handle
    }
}

// If called directly, run the setup
if (require.main === module) {
    try {
        setupPlaywright();
    } catch (error) {
        process.exit(1);
    }
}

module.exports = { setupPlaywright };
