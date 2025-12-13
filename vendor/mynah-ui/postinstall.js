const deprecationList = [
    'Config.texts.clickFileToViewDiff will be deprecated after 5.x.x',
    'MynahUIProps.onOpenDiff will be deprecated after 5.x.x',
    'ChatItemContent.buttons will render in the order of the array starting from v5.0.0, instead of reverse order.',
    'MynahUIProps.onCodeInsertToCursorPosition will be deprecated after 5.x.x',
    'MynahUIProps.onCopyCodeToClipboard will be changed to be used only on keyboard and context menu copy actions after 5.x.x',
];

deprecationList.forEach((deprecationItem) => console.log(deprecationItem));
