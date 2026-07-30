declare const everything: {
  /** Echoes back the input string */
  echo(params: {
    /** Message to echo */
    message: string;
  }): Promise<unknown>;
  /** Demonstrates how annotations can be used to provide metadata about content. */
  "get-annotated-message"(params: {
    /** Whether to include an example image */
    includeImage?: boolean;
    /** Type of message to demonstrate different annotation patterns */
    messageType: "error" | "success" | "debug";
  }): Promise<unknown>;
  /** Alias for get-annotated-message. */
  get_annotated_message(params: {
    /** Whether to include an example image */
    includeImage?: boolean;
    /** Type of message to demonstrate different annotation patterns */
    messageType: "error" | "success" | "debug";
  }): Promise<unknown>;
  /** Returns all environment variables, helpful for debugging MCP server configuration */
  "get-env"(): Promise<unknown>;
  /** Alias for get-env. */
  get_env(): Promise<unknown>;
  /** Returns up to ten resource links that reference different types of resources */
  "get-resource-links"(params?: {
    /** Number of resource links to return (1-10) */
    count?: number;
  }): Promise<unknown>;
  /** Alias for get-resource-links. */
  get_resource_links(params?: {
    /** Number of resource links to return (1-10) */
    count?: number;
  }): Promise<unknown>;
  /** Returns a resource reference that can be used by MCP clients */
  "get-resource-reference"(params?: {
    /** ID of the text resource to fetch */
    resourceId?: number;
    resourceType?: "Text" | "Blob";
  }): Promise<unknown>;
  /** Alias for get-resource-reference. */
  get_resource_reference(params?: {
    /** ID of the text resource to fetch */
    resourceId?: number;
    resourceType?: "Text" | "Blob";
  }): Promise<unknown>;
  /** Returns structured content along with an output schema for client data validation */
  "get-structured-content"(params: {
    /** Choose city */
    location: "New York" | "Chicago" | "Los Angeles";
  }): Promise<unknown>;
  /** Alias for get-structured-content. */
  get_structured_content(params: {
    /** Choose city */
    location: "New York" | "Chicago" | "Los Angeles";
  }): Promise<unknown>;
  /** Returns the sum of two numbers */
  "get-sum"(params: {
    /** First number */
    a: number;
    /** Second number */
    b: number;
  }): Promise<unknown>;
  /** Alias for get-sum. */
  get_sum(params: {
    /** First number */
    a: number;
    /** Second number */
    b: number;
  }): Promise<unknown>;
  /** Returns a tiny MCP logo image. */
  "get-tiny-image"(): Promise<unknown>;
  /** Alias for get-tiny-image. */
  get_tiny_image(): Promise<unknown>;
  /** Compresses a single file using gzip compression. Depending upon the selected output type, returns either the compressed data as a gzipped resource or a resource link, allowing it to be downloaded in a subsequent request during the current session. */
  "gzip-file-as-resource"(params?: {
    /** URL or data URI of the file content to compress */
    data?: string;
    /** Name of the output file */
    name?: string;
    /** How the resulting gzipped file should be returned. 'resourceLink' returns a link to a resource that can be read later, 'resource' returns a full resource object. */
    outputType?: "resourceLink" | "resource";
  }): Promise<unknown>;
  /** Alias for gzip-file-as-resource. */
  gzip_file_as_resource(params?: {
    /** URL or data URI of the file content to compress */
    data?: string;
    /** Name of the output file */
    name?: string;
    /** How the resulting gzipped file should be returned. 'resourceLink' returns a link to a resource that can be read later, 'resource' returns a full resource object. */
    outputType?: "resourceLink" | "resource";
  }): Promise<unknown>;
  /** Toggles simulated, random-leveled logging on or off. */
  "toggle-simulated-logging"(): Promise<unknown>;
  /** Alias for toggle-simulated-logging. */
  toggle_simulated_logging(): Promise<unknown>;
  /** Toggles simulated resource subscription updates on or off. */
  "toggle-subscriber-updates"(): Promise<unknown>;
  /** Alias for toggle-subscriber-updates. */
  toggle_subscriber_updates(): Promise<unknown>;
  /** Demonstrates a long running operation with progress updates. */
  "trigger-long-running-operation"(params?: {
    /** Duration of the operation in seconds */
    duration?: number;
    /** Number of steps in the operation */
    steps?: number;
  }): Promise<unknown>;
  /** Alias for trigger-long-running-operation. */
  trigger_long_running_operation(params?: {
    /** Duration of the operation in seconds */
    duration?: number;
    /** Number of steps in the operation */
    steps?: number;
  }): Promise<unknown>;
  /** Simulates a deep research operation that gathers, analyzes, and synthesizes information. Demonstrates MCP task-based operations with progress through multiple stages. If 'ambiguous' is true and client supports elicitation, sends an elicitation request for clarification. */
  "simulate-research-query"(params: {
    /** Simulate an ambiguous query that requires clarification (triggers input_required status) */
    ambiguous?: boolean;
    /** The research topic to investigate */
    topic: string;
  }): Promise<unknown>;
  /** Alias for simulate-research-query. */
  simulate_research_query(params: {
    /** Simulate an ambiguous query that requires clarification (triggers input_required status) */
    ambiguous?: boolean;
    /** The research topic to investigate */
    topic: string;
  }): Promise<unknown>;
};
