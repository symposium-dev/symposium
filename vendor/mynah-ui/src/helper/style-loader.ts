/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

export class StyleLoader {
  private readonly loadStyles: boolean;
  private static instance: StyleLoader | undefined;
  private constructor (loadStyles: boolean) {
    this.loadStyles = loadStyles;
  }

  public load = async (stylePath: string): Promise<void> => {
    if (this.loadStyles) {
      try {
        // Create a require context for all files in the styles directory WITH subdirectories
        const context = require.context('../styles/', true, /\.scss$/);

        // Normalize the path to ensure it starts with './'
        const normalizedPath = stylePath.startsWith('./') ? stylePath : `./${stylePath}`;

        // Use the context to import the file
        await context(normalizedPath);
      } catch (error) {
      }
    }
  };

  public static getInstance (loadStyles?: boolean): StyleLoader {
    if (StyleLoader.instance === undefined) {
      StyleLoader.instance = new StyleLoader(loadStyles ?? true);
    }

    return StyleLoader.instance;
  }

  public destroy = (): void => {
    StyleLoader.instance = undefined;
  };
}
