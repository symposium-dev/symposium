import { TreeNode, fileListToTree } from '../file-tree';

describe('file tree', () => {
  it('fileListToTree', () => {
    const modifiedFilePaths = [ 'project/src/hello.js' ];
    const deletedFilePaths = [ 'project/src/goodbye.js' ];
    const correctTreeNode: TreeNode = {
      name: 'Changes',
      type: 'folder',
      children: [
        {
          name: 'project',
          type: 'folder',
          children: [
            {
              name: 'src',
              type: 'folder',
              children: [
                { name: 'hello.js', type: 'file', filePath: 'project/src/hello.js', originalFilePath: 'project/src/hello.js', deleted: false },
                { name: 'goodbye.js', type: 'file', filePath: 'project/src/goodbye.js', originalFilePath: 'project/src/goodbye.js', deleted: true }
              ]
            }
          ]
        }
      ]
    };

    expect(fileListToTree(modifiedFilePaths, deletedFilePaths)).toEqual(correctTreeNode);
  });
});
