#!/usr/bin/env node

/**
 * Script to detect and return the appropriate Playwright version
 * Priority: local installation > package.json > latest
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

function getPlaywrightVersion() {
    try {
        // 1. Try to get locally installed version
        try {
            const localVersion = execSync('playwright --version', { encoding: 'utf8', stdio: 'pipe' });
            const versionMatch = localVersion.match(/Version (\d+\.\d+\.\d+)/);
            if (versionMatch) {
                console.log(`Found local Playwright version: ${versionMatch[1]}`);
                return versionMatch[1];
            }
        } catch (error) {
            console.log('No local Playwright installation found');
        }

        // 2. Try to get version from ui-tests package.json
        const uiTestsPackagePath = path.join(__dirname, '../ui-tests/package.json');
        if (fs.existsSync(uiTestsPackagePath)) {
            const packageJson = JSON.parse(fs.readFileSync(uiTestsPackagePath, 'utf8'));

            // Check both playwright and @playwright/test dependencies
            const playwrightVersion = packageJson.devDependencies?.playwright || packageJson.dependencies?.playwright;
            const playwrightTestVersion =
                packageJson.devDependencies?.['@playwright/test'] || packageJson.dependencies?.['@playwright/test'];

            // Prefer @playwright/test version if available, otherwise use playwright
            const version = playwrightTestVersion || playwrightVersion;

            if (version) {
                // Remove ^ or ~ prefix and get clean version
                const cleanVersion = version.replace(/[\^~]/, '');
                const sourcePackage = playwrightTestVersion ? '@playwright/test' : 'playwright';
                console.log(`Found ${sourcePackage} version in ui-tests package.json: ${cleanVersion}`);
                return cleanVersion;
            }
        }

        // 3. Fallback to latest
        console.log('No specific version found, using latest');
        return 'latest';
    } catch (error) {
        console.error('Error detecting Playwright version:', error.message);
        return 'latest';
    }
}

// If called directly, output the version
if (require.main === module) {
    console.log(getPlaywrightVersion());
}

module.exports = { getPlaywrightVersion };
