// This file runs in the webview context (browser environment)
import { MynahUI } from "@aws/mynah-ui";

// Initialize mynah-ui when the DOM is ready
const mynahUI = new MynahUI({
  rootSelector: "#mynah-root",
  loadStyles: true,
  config: {
    texts: {
      mainTitle: "Symposium",
      noTabsOpen: "### Join the symposium by opening a tab",
    },
  },
  defaults: {
    store: {
      tabTitle: "Symposium",
    },
  },
});

console.log("MynahUI initialized:", mynahUI);
