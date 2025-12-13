#!/usr/bin/env node

/**
 * Script to test WebKit browser availability and functionality
 */

const { execSync } = require('child_process');
const path = require('path');

function testWebKit() {
    try {
        console.log('Testing WebKit browser availability...');

        const uiTestsPath = path.join(__dirname, '../ui-tests');

        // Check if WebKit is installed
        console.log('Checking WebKit installation...');
        execSync('npx playwright install webkit --with-deps', {
            stdio: 'inherit',
            cwd: uiTestsPath,
        });

        // Run a simple WebKit test
        console.log('Running WebKit test...');
        execSync('npx playwright test --project=webkit --grep "should render initial data"', {
            stdio: 'inherit',
            cwd: uiTestsPath,
        });

        console.log('WebKit test completed successfully!');
        return true;
    } catch (error) {
        console.error('WebKit test failed:', error.message);
        return false;
    }
}

// If called directly, run the test
if (require.main === module) {
    const success = testWebKit();
    process.exit(success ? 0 : 1);
}

module.exports = { testWebKit };
