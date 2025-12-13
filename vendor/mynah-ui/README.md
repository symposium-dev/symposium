
# Mynah UI
> *A Data & Event Driven Chat Interface Library for Browsers and Webviews*

[![PR](https://github.com/aws/mynah-ui/actions/workflows/new_pr.yml/badge.svg?branch=main)](https://github.com/aws/mynah-ui/actions/workflows/new_pr.yml)
[![Beta](https://github.com/aws/mynah-ui/actions/workflows/beta.yml/badge.svg?branch=main)](https://github.com/aws/mynah-ui/actions/workflows/beta.yml)
[![Publish](https://github.com/aws/mynah-ui/actions/workflows/publish.yml/badge.svg?branch=main)](https://github.com/aws/mynah-ui/actions/workflows/publish.yml)
[![Deploy](https://github.com/aws/mynah-ui/actions/workflows/deploy.yml/badge.svg?branch=main)](https://github.com/aws/mynah-ui/actions/workflows/deploy.yml)

**Mynah UI** is a **_data and event_** driven chat interface designed for browsers and webviews on IDEs or any platform supporting the latest web technologies. It is utilized by Amazon Q for [VSCode](https://marketplace.visualstudio.com/items?itemName=AmazonWebServices.aws-toolkit-vscode), [JetBrains](https://plugins.jetbrains.com/plugin/11349-aws-toolkit--amazon-q-codewhisperer-and-more), [Visual studio](https://marketplace.visualstudio.com/items?itemName=AmazonWebServices.AWSToolkitforVisualStudio2022&ssr=false#overview) and [Eclipse](https://marketplace.eclipse.org/content/amazon-q).

Mynah UI operates independently of any framework or UI library, enabling seamless integration into any web-based project. This design choice ensures high configurability for theming, supporting various use cases. It functions as a standalone solution, requiring only a designated rendering location within the DOMTree.

## Table of contents
- [Quick links](#quick-links)
- [Setup, configuration and use](#setup-configuration-and-use)
  - [Guides and documentation](#guides-and-documentation)
  - [Preview](#preview)
- [Supported Browsers](#supported-browsers)
- [Security](#security)
- [License](#license)

### Quick links
* [Live Demo](https://aws.github.io/mynah-ui/)
* [API Docs](https://aws.github.io/mynah-ui/api-doc/index.html)


### Setup, configuration and use

To set up your local development environment quickly, run the following command:

```bash
npm run dev
```

This command will:
1. **Clean**: Remove existing `dist` and `node_modules` directories to ensure you're working with a fresh environment.
2. **Install**: Reinstall all necessary dependencies for both the main project and the example project.
3. **Build**: Compile the project using Webpack in production mode.
4. **Start Example**: Install dependencies and build the example project, then start the development server with `watch` mode enabled. The project will be served on `localhost:9000` using `live-server`.
5. **Watch**: Start the main project in `watch` mode.
After running this command, any changes you make will automatically rebuild and refresh your development environment, allowing you to work seamlessly.


#### Guides and documentation
Please refer to the following guides:

* [Startup guide](./docs/STARTUP.md)
* [Constructor properties](./docs/PROPERTIES.md)
* [Configuration](./docs/CONFIG.md)
* [Data model](./docs/DATAMODEL.md)
* [Usage](./docs/USAGE.md)
* [Styling](./docs/STYLING.md)
* [Testing](./docs/TESTING.md)
* [Developer guidelines (contribution)](./docs/DEVELOPER.md)

#### Preview
![Preview](./docs/img/splash.gif)

### Supported Browsers

**Mynah UI** <em>- due to its extensive CSS structure -</em> supports only evergreen browsers, including WebKit-based WebUI renderers.

## Security

See [CONTRIBUTING](CONTRIBUTING.md#security-issue-notifications) for more information.

# License
[Apache 2.0 License.](LICENSE)
