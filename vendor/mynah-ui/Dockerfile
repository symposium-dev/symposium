# Version-agnostic Dockerfile for Mynah UI E2E Tests
# Supports dynamic Playwright version detection
ARG PLAYWRIGHT_VERSION=latest
FROM mcr.microsoft.com/playwright:${PLAYWRIGHT_VERSION}

# Set working directory
WORKDIR /app

# Copy the src from the root
COPY ./src /app/src

# Copy config files from root
COPY ./package.json /app
COPY ./package-lock.json /app
COPY ./postinstall.js /app
COPY ./webpack.config.js /app
COPY ./tsconfig.json /app

# Copy scripts directory for version-agnostic setup
COPY ./scripts /app/scripts

# Copy required files from ui-tests
COPY ./ui-tests/package.json /app/ui-tests/
COPY ./ui-tests/playwright.config.ts /app/ui-tests/
COPY ./ui-tests/tsconfig.json /app/ui-tests/
COPY ./ui-tests/webpack.config.js /app/ui-tests/

# Copy the directories from ui-tests
COPY ./ui-tests/__test__ /app/ui-tests/__test__
COPY ./ui-tests/src /app/ui-tests/src
COPY ./ui-tests/__snapshots__ /app/ui-tests/__snapshots__

# Install dependencies and build MynahUI
RUN npm install
RUN npm run build

# Setup Playwright with version-agnostic approach
RUN cd ./ui-tests && node ../scripts/setup-playwright.js && npm run prepare

# Ensure all browsers are installed with dependencies
RUN cd ./ui-tests && npx playwright install --with-deps

# Run health check to verify installation
RUN cd ./ui-tests && node ../scripts/docker-health-check.js

# Set environment variables for WebKit
ENV WEBKIT_FORCE_COMPLEX_TEXT=0
ENV WEBKIT_DISABLE_COMPOSITING_MODE=1

# Default command to run the tests
CMD ["sh", "-c", "cd ./ui-tests && npm run e2e${BROWSER:+:$BROWSER}"]
