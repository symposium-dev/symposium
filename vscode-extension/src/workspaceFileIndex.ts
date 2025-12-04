import * as vscode from "vscode";
import * as cp from "child_process";
import { logger } from "./extension";

/** Maximum number of files to include in context commands */
const MAX_CONTEXT_FILES = 16000;

/** Maximum number of symbols to include in context commands */
const MAX_CONTEXT_SYMBOLS = 5000;

/** Represents a workspace symbol for context */
export interface ContextSymbol {
  name: string;
  kind: vscode.SymbolKind;
  location: string; // relative file path
  containerName?: string;
  /** Full definition range (from DocumentSymbol.range) */
  range: {
    startLine: number;
    startChar: number;
    endLine: number;
    endChar: number;
  };
  /** Selection range - just the symbol name (from DocumentSymbol.selectionRange) */
  selectionRange?: {
    startLine: number;
    startChar: number;
    endLine: number;
    endChar: number;
  };
}

/**
 * Maintains a live index of files and symbols in the workspace.
 *
 * Files:
 * - Initializes from `git ls-files` (respects .gitignore)
 * - Falls back to `workspace.findFiles` for non-git workspaces
 * - Uses FileSystemWatcher for live updates
 * - Tracks open editor tabs (even files outside workspace)
 *
 * Symbols:
 * - Fetched via executeWorkspaceSymbolProvider with empty query
 * - Results depend on language server support
 */
export class WorkspaceFileIndex {
  #workspaceFolder: vscode.WorkspaceFolder;
  #files: Set<string> = new Set();
  #symbols: ContextSymbol[] = [];
  #watcher: vscode.FileSystemWatcher | undefined;
  #openTabsDisposable: vscode.Disposable | undefined;
  #onDidChange = new vscode.EventEmitter<void>();
  #isGitRepo: boolean = false;

  /** Fires when the file or symbol list changes */
  readonly onDidChange = this.#onDidChange.event;

  constructor(workspaceFolder: vscode.WorkspaceFolder) {
    this.#workspaceFolder = workspaceFolder;
  }

  /** Initialize the index - call this before using */
  async initialize(): Promise<void> {
    // Try git ls-files first
    this.#isGitRepo = await this.#tryGitLsFiles();

    if (!this.#isGitRepo) {
      // Fall back to workspace.findFiles
      await this.#fallbackFindFiles();
    }

    // Set up file watcher for live updates
    this.#setupWatcher();

    // Track open tabs
    this.#setupOpenTabsTracking();

    // Fetch workspace symbols (async, non-blocking)
    this.#fetchWorkspaceSymbols();

    logger.debug("fileIndex", "Initialized workspace file index", {
      workspace: this.#workspaceFolder.name,
      fileCount: this.#files.size,
      isGitRepo: this.#isGitRepo,
    });
  }

  /** Get all indexed files as relative paths */
  getFiles(): string[] {
    // Combine workspace files with open tabs, limit to MAX_CONTEXT_FILES
    const allFiles = new Set(this.#files);

    // Add open tabs (may include files outside workspace)
    for (const tabGroup of vscode.window.tabGroups.all) {
      for (const tab of tabGroup.tabs) {
        if (tab.input instanceof vscode.TabInputText) {
          const uri = tab.input.uri;
          const relativePath = this.#getRelativePath(uri);
          if (relativePath) {
            allFiles.add(relativePath);
          }
        }
      }
    }

    // Convert to sorted array and limit
    const sorted = Array.from(allFiles).sort();
    if (sorted.length > MAX_CONTEXT_FILES) {
      logger.debug("fileIndex", "Truncating file list", {
        total: sorted.length,
        limit: MAX_CONTEXT_FILES,
      });
      return sorted.slice(0, MAX_CONTEXT_FILES);
    }
    return sorted;
  }

  /** Get all indexed symbols */
  getSymbols(): ContextSymbol[] {
    return this.#symbols;
  }

  /** Get the workspace folder this index is for */
  get workspaceFolder(): vscode.WorkspaceFolder {
    return this.#workspaceFolder;
  }

  /** Fetch workspace symbols using DocumentSymbol for full definition ranges */
  async #fetchWorkspaceSymbols(): Promise<void> {
    try {
      const startTime = Date.now();
      const files = this.getFiles();

      // Filter to source files that are likely to have symbols
      const sourceExtensions = new Set([
        "ts",
        "tsx",
        "js",
        "jsx",
        "rs",
        "py",
        "go",
        "java",
        "c",
        "cpp",
        "h",
        "hpp",
        "cs",
        "rb",
        "swift",
        "kt",
        "scala",
        "vue",
        "svelte",
      ]);

      const sourceFiles = files.filter((f) => {
        const ext = f.split(".").pop()?.toLowerCase();
        return ext && sourceExtensions.has(ext);
      });

      logger.debug("fileIndex", "Fetching DocumentSymbols for source files", {
        totalFiles: files.length,
        sourceFiles: sourceFiles.length,
      });

      const contextSymbols: ContextSymbol[] = [];
      let filesProcessed = 0;
      let filesWithSymbols = 0;

      // Process files in parallel batches for performance
      const batchSize = 10;
      for (let i = 0; i < sourceFiles.length; i += batchSize) {
        if (contextSymbols.length >= MAX_CONTEXT_SYMBOLS) {
          break;
        }

        const batch = sourceFiles.slice(i, i + batchSize);
        const results = await Promise.all(
          batch.map((relativePath) => this.#fetchDocumentSymbols(relativePath)),
        );

        for (let j = 0; j < results.length; j++) {
          const symbols = results[j];
          const relativePath = batch[j];
          filesProcessed++;

          if (symbols && symbols.length > 0) {
            filesWithSymbols++;
            this.#collectSymbols(
              symbols,
              relativePath,
              contextSymbols,
              undefined,
            );
          }

          if (contextSymbols.length >= MAX_CONTEXT_SYMBOLS) {
            break;
          }
        }
      }

      this.#symbols = contextSymbols;

      const elapsed = Date.now() - startTime;
      logger.debug("fileIndex", "Fetched DocumentSymbols", {
        filesProcessed,
        filesWithSymbols,
        symbolCount: contextSymbols.length,
        elapsed,
      });

      // Notify listeners
      if (contextSymbols.length > 0) {
        this.#onDidChange.fire();
      }
    } catch (err) {
      logger.error("fileIndex", "Failed to fetch workspace symbols", {
        error: err,
      });
    }
  }

  /** Fetch DocumentSymbol for a single file */
  async #fetchDocumentSymbols(
    relativePath: string,
  ): Promise<vscode.DocumentSymbol[] | null> {
    try {
      const uri = vscode.Uri.joinPath(this.#workspaceFolder.uri, relativePath);
      const symbols = await vscode.commands.executeCommand<
        vscode.DocumentSymbol[]
      >("vscode.executeDocumentSymbolProvider", uri);
      return symbols || null;
    } catch {
      // File might not exist or no language server available
      return null;
    }
  }

  /** Recursively collect symbols from DocumentSymbol tree */
  #collectSymbols(
    symbols: vscode.DocumentSymbol[],
    relativePath: string,
    output: ContextSymbol[],
    parentName: string | undefined,
  ): void {
    for (const sym of symbols) {
      if (output.length >= MAX_CONTEXT_SYMBOLS) {
        return;
      }

      // Build container name from parent
      const containerName = parentName;

      output.push({
        name: sym.name,
        kind: sym.kind,
        location: relativePath,
        containerName,
        range: {
          startLine: sym.range.start.line,
          startChar: sym.range.start.character,
          endLine: sym.range.end.line,
          endChar: sym.range.end.character,
        },
        selectionRange: {
          startLine: sym.selectionRange.start.line,
          startChar: sym.selectionRange.start.character,
          endLine: sym.selectionRange.end.line,
          endChar: sym.selectionRange.end.character,
        },
      });

      // Recurse into children
      if (sym.children && sym.children.length > 0) {
        const childParent = parentName
          ? `${parentName}::${sym.name}`
          : sym.name;
        this.#collectSymbols(sym.children, relativePath, output, childParent);
      }
    }
  }

  /** Try to populate from git ls-files */
  async #tryGitLsFiles(): Promise<boolean> {
    return new Promise((resolve) => {
      const cwd = this.#workspaceFolder.uri.fsPath;

      cp.exec(
        "git ls-files",
        { cwd, maxBuffer: 10 * 1024 * 1024 },
        (error, stdout) => {
          if (error) {
            logger.debug("fileIndex", "git ls-files failed, will use fallback", {
              error: error.message,
            });
            resolve(false);
            return;
          }

          const files = stdout
            .split("\n")
            .map((f) => f.trim())
            .filter((f) => f.length > 0);

          for (const file of files) {
            this.#files.add(file);
          }

          resolve(true);
        },
      );
    });
  }

  /** Fallback: use workspace.findFiles for non-git workspaces */
  async #fallbackFindFiles(): Promise<void> {
    // Use a reasonable exclude pattern
    const excludePattern =
      "**/node_modules/**,**/.git/**,**/target/**,**/dist/**,**/build/**";

    const uris = await vscode.workspace.findFiles(
      new vscode.RelativePattern(this.#workspaceFolder, "**/*"),
      excludePattern,
      MAX_CONTEXT_FILES,
    );

    for (const uri of uris) {
      const relativePath = this.#getRelativePath(uri);
      if (relativePath) {
        this.#files.add(relativePath);
      }
    }
  }

  /** Set up file watcher for live updates */
  #setupWatcher(): void {
    // Watch all files in workspace
    this.#watcher = vscode.workspace.createFileSystemWatcher(
      new vscode.RelativePattern(this.#workspaceFolder, "**/*"),
    );

    this.#watcher.onDidCreate((uri) => {
      const relativePath = this.#getRelativePath(uri);
      if (relativePath && this.#shouldIncludeFile(relativePath)) {
        this.#files.add(relativePath);
        logger.debug("fileIndex", "File created", { path: relativePath });
        this.#onDidChange.fire();
      }
    });

    this.#watcher.onDidDelete((uri) => {
      const relativePath = this.#getRelativePath(uri);
      if (relativePath && this.#files.has(relativePath)) {
        this.#files.delete(relativePath);
        logger.debug("fileIndex", "File deleted", { path: relativePath });
        this.#onDidChange.fire();
      }
    });

    // Note: We don't listen to onDidChange (file content changes) as that
    // doesn't affect the file list
  }

  /** Set up tracking for open editor tabs */
  #setupOpenTabsTracking(): void {
    this.#openTabsDisposable = vscode.window.tabGroups.onDidChangeTabs(() => {
      // When tabs change, the file list might include new external files
      this.#onDidChange.fire();
    });
  }

  /** Get relative path for a URI, or undefined if outside workspace */
  #getRelativePath(uri: vscode.Uri): string | undefined {
    // Check if it's within the workspace
    const workspacePath = this.#workspaceFolder.uri.fsPath;
    const filePath = uri.fsPath;

    if (filePath.startsWith(workspacePath)) {
      // Inside workspace - return relative path
      let relative = filePath.slice(workspacePath.length);
      if (relative.startsWith("/") || relative.startsWith("\\")) {
        relative = relative.slice(1);
      }
      return relative;
    } else {
      // Outside workspace - return full path
      return filePath;
    }
  }

  /** Check if a file should be included (basic filtering for non-git) */
  #shouldIncludeFile(relativePath: string): boolean {
    // For git repos, git ls-files already filters
    if (this.#isGitRepo) {
      // But watcher might catch files not in git - check common excludes
      const excludePatterns = [
        /node_modules\//,
        /\.git\//,
        /target\//,
        /dist\//,
        /build\//,
        /\.DS_Store$/,
      ];
      return !excludePatterns.some((p) => p.test(relativePath));
    }
    return true;
  }

  /** Dispose of resources */
  dispose(): void {
    this.#watcher?.dispose();
    this.#openTabsDisposable?.dispose();
    this.#onDidChange.dispose();
  }
}
