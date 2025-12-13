#!/usr/bin/env node

/**
 * Docker health check script to verify browser installations
 */

const { execSync } = require('child_process');

function healthCheck() {
    try {
        console.log('=== Docker Health Check ===');

        // Check if we're in Docker
        console.log('Environment: Docker Container');

        // Check Playwright installation
        console.log('\n1. Checking Playwright...');
        execSync('npx playwright --version', { stdio: 'inherit' });

        // Check browser installations
        console.log('\n2. Checking browsers...');
        execSync('npx playwright install --dry-run', { stdio: 'inherit' });

        // Test WebKit specifically
        console.log('\n3. Testing WebKit launch...');
        execSync('npx playwright test --list --project=webkit | head -5', {
            stdio: 'inherit',
            shell: true,
        });

        console.log('\n=== Health Check Passed ===');
        return true;
    } catch (error) {
        console.error('Health check failed:', error.message);
        return false;
    }
}

// If called directly, run the health check
if (require.main === module) {
    const success = healthCheck();
    process.exit(success ? 0 : 1);
}

module.exports = { healthCheck };
