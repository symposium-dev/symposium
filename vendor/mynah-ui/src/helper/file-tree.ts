/*!
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { MynahIcons, MynahIconsType } from '../main';
import { FileNodeAction, Status, TreeNodeDetails } from '../static';
import { Config } from './config';

export type TreeNode = FolderNode | FileNode;
export interface FileNode {
  name: string;
  type: 'file';
  filePath: string;
  originalFilePath: string;
  deleted: boolean;
  actions?: FileNodeAction[];
  details?: TreeNodeDetails;
}
export interface FolderNode {
  name: string;
  type: 'folder';
  details?: TreeNodeDetails;
  children: Array<FolderNode | FileNode>;
}

/*
 * Converts a list of file Paths into a tree
 *
 * @input: The list of path in string format
 * Example Input:
 *  modifiedFilePaths: [
 *   "project/src/hello.js",
 * ]
 *  deletedFilePaths: [
 *   "project/src/goodbye.js",
 *  ]
 *
 * Example output:
 * {
 *  name: 'Changes',
 *  type: 'folder',
 *  children: [{
 *      name: 'project',
 *      type: 'folder',
 *      children: [{
 *          name: 'src',
 *          type: 'folder',
 *          children: [
 *              { name: 'hello.js', type: 'file', filePath: 'project/src/hello.js', deleted: false },
 *              { name: 'goodbye.js', type: 'file', filePath: 'project/src/goodbye.js', deleted: true },
 *          ]
 *      }]
 *  }]
 * }
 */
export const fileListToTree = (
  modifiedFilePaths: string[],
  deletedFilePaths: string[] = [],
  actions?: Record<string, FileNodeAction[]>,
  details?: Record<string, TreeNodeDetails>,
  rootTitle?: string,
  rootStatusIcon?: MynahIcons | MynahIconsType,
  rootStatusIconForegroundStatus?: Status,
  rootLabel?: string,
): TreeNode => {
  return [ ...splitFilePaths(modifiedFilePaths, false), ...splitFilePaths(deletedFilePaths, true) ].reduce<TreeNode>(
    (acc, { originalFilePath, filePath, deleted }) => {
      let currentNode = acc;
      const currentPathParts: string[] = [];

      for (let i = 0; i < filePath.length; i++) {
        const fileOrFolder = filePath[i];
        currentPathParts.push(fileOrFolder);
        const currentFullPath = currentPathParts.join('/');

        if (i === filePath.length - 1) {
          // Handle file (last item in path)
          (currentNode as FolderNode).children?.push({
            type: 'file',
            name: fileOrFolder,
            filePath: currentFullPath,
            originalFilePath,
            deleted,
            actions: actions?.[originalFilePath],
            details: details?.[originalFilePath],
          });
          break;
        } else {
          // Handle folder
          const oldItem = (currentNode as FolderNode).children?.find(
            (item) =>
              item.type === 'folder' &&
              item.name === fileOrFolder &&
              // Compare the current path depth
              item.children.some(child =>
                child.type === 'file'
                  ? child.filePath.startsWith(currentFullPath + '/')
                  : true
              )
          );

          if (oldItem != null) {
            currentNode = oldItem;
          } else {
            const newItem: FolderNode = {
              name: fileOrFolder,
              type: 'folder',
              children: [],
            };
            (currentNode as FolderNode).children?.push(newItem);
            currentNode = newItem;
          }
        }
      }
      return acc;
    },
    {
      name: rootTitle ?? Config.getInstance().config.texts.changes,
      type: 'folder',
      children: [],
      ...(rootLabel != null || rootStatusIcon != null || rootStatusIconForegroundStatus != null
        ? {
            details: {
              label: rootLabel,
              icon: rootStatusIcon,
              iconForegroundStatus: rootStatusIconForegroundStatus,
            }
          }
        : {})
    }
  );
};

const splitFilePaths = (paths: string[], deleted: boolean): Array<{ originalFilePath: string; filePath: string[]; deleted: boolean }> =>
  paths
    // split file path by folder. ignore dot folders
    .map(filePath => ({ originalFilePath: filePath, filePath: filePath.split('/').filter(item => item !== undefined && item !== '.'), deleted }));
