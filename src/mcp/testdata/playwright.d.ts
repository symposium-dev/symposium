declare const playwright: {
  /** Close the page */
  browser_close(): Promise<unknown>;
  /** Resize the browser window */
  browser_resize(params: {
    /** Height of the browser window */
    height: number;
    /** Width of the browser window */
    width: number;
  }): Promise<unknown>;
  /** Returns all console messages */
  browser_console_messages(params: {
    /** Return all console messages since the beginning of the session, not just since the last navigation. Defaults to false. */
    all?: boolean;
    /** Filename to save the console messages to. If not provided, messages are returned as text. */
    filename?: string;
    /** Level of the console messages to return. Each level includes the messages of more severe levels. Defaults to "info". */
    level: "error" | "warning" | "info" | "debug";
  }): Promise<unknown>;
  /** Handle a dialog */
  browser_handle_dialog(params: {
    /** Whether to accept the dialog. */
    accept: boolean;
    /** The text of the prompt in case of a prompt dialog. */
    promptText?: string;
  }): Promise<unknown>;
  /** Evaluate JavaScript expression on page or element */
  browser_evaluate(params: {
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Filename to save the result to. If not provided, result is returned as text. */
    filename?: string;
    /** () => { /* code * / } or (element) => { /* code * / } when element is provided */
    function: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target?: string;
  }): Promise<unknown>;
  /** Upload one or multiple files */
  browser_file_upload(params?: {
    /** The absolute paths to the files to upload. Can be single file or multiple files. If omitted, file chooser is cancelled. */
    paths?: string[];
  }): Promise<unknown>;
  /** Drop files or MIME-typed data onto an element, as if dragged from outside the page. At least one of "paths" or "data" must be provided. */
  browser_drop(params: {
    /** Data to drop, as a map of MIME type to string value (e.g. {"text/plain": "hello", "text/uri-list": "https://example.com"}). */
    data?: Record<string, string>;
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Absolute paths to files to drop onto the element. */
    paths?: string[];
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target: string;
  }): Promise<unknown>;
  /** Search the accessibility snapshot of the current page for text or a regular expression. Returns matching snapshot nodes with a few lines of surrounding context (like search snippets), each shown under its path from the root of the tree, which is cheaper than capturing the whole snapshot when you only need to locate an element and its ref. */
  browser_find(params?: {
    /** Regular expression to search for in the page snapshot. Matching is case-sensitive by default; wrap the pattern in slashes to add flags, e.g. "/error/i" for case-insensitive. Provide either text or regex, not both. */
    regex?: string;
    /** Plain text to search for in the page snapshot (case-insensitive substring match). Provide either text or regex, not both. */
    text?: string;
  }): Promise<unknown>;
  /** Fill multiple form fields */
  browser_fill_form(params: {
    /** Fields to fill in */
    fields: ({
      /** Human-readable element description used to obtain permission to interact with the element */
      element?: string;
      /** Human-readable field name */
      name: string;
      /** Exact target element reference from the page snapshot, or a unique element selector */
      target: string;
      /** Type of the field */
      type: "textbox" | "checkbox" | "radio" | "combobox" | "slider";
      /** Value to fill in the field. If the field is a checkbox, the value should be `true` or `false`. If the field is a combobox, the value should be the text of the option. */
      value: string;
    })[];
  }): Promise<unknown>;
  /** Press a key on the keyboard */
  browser_press_key(params: {
    /** Name of the key to press or a character to generate, such as `ArrowLeft` or `a` */
    key: string;
  }): Promise<unknown>;
  /** Type text into editable element */
  browser_type(params: {
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Whether to type one character at a time. Useful for triggering key handlers in the page. By default entire text is filled in at once. */
    slowly?: boolean;
    /** Whether to submit entered text (press Enter after) */
    submit?: boolean;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target: string;
    /** Text to type into the element */
    text: string;
  }): Promise<unknown>;
  /** Navigate to a URL */
  browser_navigate(params: {
    /** The URL to navigate to */
    url: string;
  }): Promise<unknown>;
  /** Go back to the previous page in the history */
  browser_navigate_back(): Promise<unknown>;
  /** Returns a numbered list of network requests since loading the page. Use browser_network_request with the number to get full details. */
  browser_network_requests(params: {
    /** Filename to save the network requests to. If not provided, requests are returned as text. */
    filename?: string;
    /** Only return requests whose URL matches this regexp (e.g. "/api/.*user"). */
    filter?: string;
    /** Whether to include successful static resources like images, fonts, scripts, etc. Defaults to false. */
    static: boolean;
  }): Promise<unknown>;
  /** Returns full details (headers and body) of a single network request, or a single part if `part` is set. Use the number from browser_network_requests. */
  browser_network_request(params: {
    /** Filename to save the result to. If not provided, output is returned as text. */
    filename?: string;
    /** 1-based index of the request, as printed by browser_network_requests. */
    index: number;
    /** Return only this part of the request. Omit to return full details. */
    part?: "request-headers" | "request-body" | "response-headers" | "response-body";
  }): Promise<unknown>;
  /** Run a Playwright code snippet. Unsafe: executes arbitrary JavaScript in the Playwright server process and is RCE-equivalent. */
  browser_run_code_unsafe(params?: {
    /** A JavaScript function containing Playwright code to execute. It will be invoked with a single argument, page, which you can use for any page interaction. For example: `async (page) => { await page.getByRole('button', { name: 'Submit' }).click(); return await page.title(); }` */
    code?: string;
    /** Load code from the specified file. If both code and filename are provided, code will be ignored. */
    filename?: string;
  }): Promise<unknown>;
  /** Take a screenshot of the current page. You can't perform actions based on the screenshot, use browser_snapshot for actions. */
  browser_take_screenshot(params: {
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** File name to save the screenshot to. Defaults to `page-{timestamp}.{png|jpeg}` if not specified. Prefer relative file names to stay within the output directory. */
    filename?: string;
    /** When true, takes a screenshot of the full scrollable page, instead of the currently visible viewport. Cannot be used with element screenshots. */
    fullPage?: boolean;
    /** Image resolution scale. "css" produces a screenshot sized in CSS pixels (smaller, consistent across devices). "device" produces a high-resolution screenshot using device pixels (larger, accounts for the device pixel ratio). Default is css. */
    scale: "css" | "device";
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target?: string;
    /** Image format for the screenshot. Default is png. */
    type: "png" | "jpeg";
  }): Promise<unknown>;
  /** Capture accessibility snapshot of the current page, this is better than screenshot */
  browser_snapshot(params?: {
    /** Include each element's bounding box as [box=x,y,width,height] in the snapshot. Coordinates are viewport-relative, in CSS pixels (Element.getBoundingClientRect) */
    boxes?: boolean;
    /** Limit the depth of the snapshot tree */
    depth?: number;
    /** Save snapshot to markdown file instead of returning it in the response. */
    filename?: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target?: string;
  }): Promise<unknown>;
  /** Perform click on a web page */
  browser_click(params: {
    /** Button to click, defaults to left */
    button?: "left" | "right" | "middle";
    /** Whether to perform a double click instead of a single click */
    doubleClick?: boolean;
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Modifier keys to press */
    modifiers?: ("Alt" | "Control" | "ControlOrMeta" | "Meta" | "Shift")[];
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target: string;
  }): Promise<unknown>;
  /** Perform drag and drop between two elements */
  browser_drag(params: {
    /** Human-readable target element description used to obtain the permission to interact with the element */
    endElement?: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    endTarget: string;
    /** Human-readable source element description used to obtain the permission to interact with the element */
    startElement?: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    startTarget: string;
  }): Promise<unknown>;
  /** Hover over element on page */
  browser_hover(params: {
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target: string;
  }): Promise<unknown>;
  /** Select an option in a dropdown */
  browser_select_option(params: {
    /** Human-readable element description used to obtain permission to interact with the element */
    element?: string;
    /** Exact target element reference from the page snapshot, or a unique element selector */
    target: string;
    /** Array of values to select in the dropdown. This can be a single value or multiple values. */
    values: string[];
  }): Promise<unknown>;
  /** List, create, close, or select a browser tab. */
  browser_tabs(params: {
    /** Operation to perform */
    action: "list" | "new" | "close" | "select";
    /** Tab index, used for close/select. If omitted for close, current tab is closed. */
    index?: number;
    /** URL to navigate to in the new tab, used for new. */
    url?: string;
  }): Promise<unknown>;
  /** Wait for text to appear or disappear or a specified time to pass */
  browser_wait_for(params?: {
    /** The text to wait for */
    text?: string;
    /** The text to wait for to disappear */
    textGone?: string;
    /** The time to wait in seconds */
    time?: number;
  }): Promise<unknown>;
};
