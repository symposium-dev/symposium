const openFile = async () => {
    try {
        // Use dynamic import to load the ES module
        const open = (await import('open')).default;
        await open('./dist/index.html');
    } catch (err) {
        console.error('Error opening file:', err);
    }
};

openFile();
