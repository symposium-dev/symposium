var file_system = require('fs');
var mynahUIPackageJson = require('../package.json');
var archiver = require('archiver');

var output = file_system.createWriteStream('mynah-ui-demo.zip');
var archive = archiver('zip');

output.on('close', function () {
    console.log(archive.pointer() + ' total bytes');
    console.log('archiver has been finalized and the output file descriptor has closed.');
});

archive.on('error', function (err) {
    throw err;
});

archive.pipe(output);

// append files from a sub-directory, putting its contents at the root of archive
archive.directory('dist/', `mynah-ui-${mynahUIPackageJson?.version ?? '??'}`);

archive.finalize();
