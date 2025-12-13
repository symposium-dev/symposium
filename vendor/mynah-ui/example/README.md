
# Mynah UI Example (IDE look & feel)

## Project Description

Mynah UI Example provides an interactive environment to demonstrate the features and capabilities of the Mynah UI library. This example mimics an IDE look and feel to showcase live changes and dynamic updates as you modify the UI library.

## Table of Contents

- [Setup, configuration and use](#setup-configuration-and-use)
- [How to use the project](#how-to-use-the-project)
- [Supported browsers](#supported-browsers)
- [Additional information](#additional-information)

### Setup, configuration and use

To start the example, simply run the following script through your terminal in the root folder.

>[!TIP]
> Local environment quick start
 ```bash
 # local git already cloned
 npm run dev
```

After the script runs, open your browser and go to `http://localhost:9000` to see the example.

#### Preview
![Preview](./docs/img/splash.gif)

The steps in the script are as follows:
- First, it cleans the project by removing `dist` and `node_modules` directories.
- Then, it installs the dependencies.
- After that, it builds the project.
- Finally, open `index.html` inside the `dist` folder in your browser.

### How to use the project

If you check the dependencies of the example, you'll see that the mynah-ui dependency is connected to the parent folder, which allows us to use the mynah-ui directly from the parent folder instead of an npm dependency.

If you want, you can run `mynah-ui` and example scripts in separate terminals to see their watch processes separately.

If you just need the built version (no need to watch for changes), simply run the below and open the `index.html` inside the `./example/dist` folder in your browser.

From your root folder:
```console
npm install && npm run build && cd ./example && npm install && npm run build
```

### Supported browsers

**Mynah UI** <em>-because of its extensive CSS structure-</em> only supports evergreen browsers, including WebKit-based WebUI renderers.

### Additional information

Please check the root folder [README.md](../README.md) for usage guidelines, license information, and other helpful guides.

# License
[Apache 2.0 License.](../LICENSE)
