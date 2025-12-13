#!/usr/bin/env node

/**
 * Pre-test setup script that ensures Playwright is properly configured
 * This runs before tests to guarantee version consistency
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { getPlaywrightVersion } = require('./get-playwright-version');

function checkPlaywrightInstallation() {
    const uiTestsPath = path.join(__dirname, '../ui-tests');
    const nodeModulesPath = path.join(uiTestsPath, 'node_modules');
    const playwrightPath = path.join(nodeModulesPath, '@playwright');

    return fs.existsSync(playwrightPath);
}

function getInstalledPlaywrightVersion() {
    try {
        const uiTestsPath = path.join(__dirname, '../ui-tests');
        const packageLockPath = path.join(uiTestsPath, 'package-lock.json');

        if (fs.existsSync(packageLockPath)) {
            const packageLock = JSON.parse(fs.readFileSync(packageLockPath, 'utf8'));
            return (
                packageLock.packages?.['node_modules/@playwright/test']?.version ||
                packageLock.packages?.['node_modules/playwright']?.version ||
                null
            );
        }
        return null;
    } catch (error) {
        console.warn('Could not read package-lock.json:', error.message);
        return null;
    }
}

function preTestSetup() {
    console.log('🔍 Running pre-test setup...');

    try {
        const expectedVersion = getPlaywrightVersion();
        console.log(`📋 Expected Playwright version: ${expectedVersion}`);

        const isInstalled = checkPlaywrightInstallation();
        const installedVersion = getInstalledPlaywrightVersion();

        console.log(`📦 Playwright installed: ${isInstalled}`);
        console.log(`📦 Installed version: ${installedVersion || 'unknown'}`);

        // Check if we need to install/update
        const needsSetup = !isInstalled || (expectedVersion !== 'latest' && installedVersion !== expectedVersion);

        if (needsSetup) {
            console.log('🔧 Setting up Playwright...');

            // Run setup with target directory
            const { setupPlaywright } = require('./setup-playwright');
            const uiTestsPath = path.join(__dirname, '../ui-tests');
            setupPlaywright(uiTestsPath);
        } else {
            console.log('✅ Playwright is already properly configured');
        }

        console.log('🎉 Pre-test setup completed successfully!');
    } catch (error) {
        console.error('❌ Pre-test setup failed:', error.message);
        process.exit(1);
    }
}

// If called directly, run the setup
if (require.main === module) {
    preTestSetup();
}

module.exports = { preTestSetup };
