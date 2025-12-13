import escapeHTML from 'escape-html';
import { DetailedListItem, DetailedListItemGroup, QuickActionCommand, QuickActionCommandGroup } from '../static';
import { MynahIcons } from '../main';

export const filterQuickPickItems = (commands: QuickActionCommandGroup[], searchTerm: string, hideSearchGroup?: boolean): QuickActionCommandGroup[] => {
  if (searchTerm.trim() === '') {
    return commands;
  }

  const matchedCommands: Array<{score: number; command: QuickActionCommand}> = [];

  const findMatches = (cmd: QuickActionCommand): void => {
    const score = calculateItemScore(cmd.command, searchTerm);
    if (score > 0) {
      matchedCommands.push({
        score,
        command: {
          ...cmd,
          // Update command with highlighted text
          // It is being reverted when user makes the selection
          command: highlightMatch(escapeHTML(cmd.command), searchTerm)
        }
      });
    }

    // Search for children
    cmd.children?.forEach(childGroup => {
      childGroup.commands.forEach(childCmd => {
        findMatches(childCmd);
      });
    });
  };

  // Filter all commands
  commands.forEach(group => {
    group.commands.forEach(cmd => {
      findMatches(cmd);
    });
  });

  const returnGroup: QuickActionCommandGroup = {
    icon: MynahIcons.SEARCH,
    commands: []
  };
  if (matchedCommands.length > 0) {
    returnGroup.commands = matchedCommands.sort((a, b) => (b.score ?? 0) - (a.score ?? 0))
      .map((item) => item.command);
  }
  if (hideSearchGroup !== true) { returnGroup.groupName = `### ${searchTerm}: (${returnGroup.commands.length})`; }
  return [ returnGroup ];
};

export const MARK_OPEN = '<mark>';
export const MARK_CLOSE = '</mark>';

const highlightMatch = (text: string, searchTerm: string): string => {
  const textToCompare = text.toLowerCase();
  const searchTermToCompare = searchTerm.toLowerCase();

  // Exact
  if (textToCompare === searchTermToCompare) {
    return `${MARK_OPEN}${text}${MARK_CLOSE}`;
  }

  // Prefix
  if (textToCompare.startsWith(searchTermToCompare)) {
    const matchLength = searchTerm.length;
    const matchedPart = text.slice(0, matchLength);
    const restPart = text.slice(matchLength);
    return `${MARK_OPEN}${matchedPart}${MARK_CLOSE}${restPart}`;
  }

  // Contains
  const startIndex = textToCompare.indexOf(searchTermToCompare);
  if (startIndex !== -1) {
    const before = text.slice(0, startIndex);
    const match = text.slice(startIndex, startIndex + searchTerm.length);
    const after = text.slice(startIndex + searchTerm.length);
    return `${before}${MARK_OPEN}${match}${MARK_CLOSE}${after}`;
  }

  // Words
  const words = text.split(' ');
  for (let i = 0; i < words.length; i++) {
    const word = words[i].toLowerCase();
    if (word.includes(searchTermToCompare)) {
      const startIdx = word.indexOf(searchTermToCompare);
      const originalWord = words[i];
      words[i] =
              originalWord.slice(0, startIdx) +
              `${MARK_OPEN}${originalWord.slice(startIdx, startIdx + searchTerm.length)}${MARK_CLOSE}` +
              originalWord.slice(startIdx + searchTerm.length);
      return words.join(' ');
    }
  }

  // Partial
  let result = '';
  let lastIndex = 0;
  let termIndex = 0;

  for (let i = 0; i < text.length && termIndex < searchTerm.length; i++) {
    if (text[i].toLowerCase() === searchTerm[termIndex].toLowerCase()) {
      result += text.slice(lastIndex, i);
      result += `${MARK_OPEN}${text[i]}${MARK_CLOSE}`;
      lastIndex = i + 1;
      termIndex++;
    }
  }
  result += text.slice(lastIndex);

  return termIndex === searchTerm.length ? result : text;
};

const calculateItemScore = (text: string, searchTerm: string): number => {
  const normalizedText = text.toLowerCase();
  const normalizedTerm = searchTerm.toLowerCase();

  const isExactMatch = normalizedText === normalizedTerm;
  const isPrefixMatch = normalizedText.startsWith(normalizedTerm);
  const isWordStartMatch = normalizedText.split(' ').some(word => word.startsWith(normalizedTerm));
  const isContainsMatch = normalizedText.includes(normalizedTerm);

  if (isExactMatch) return 100;
  if (isPrefixMatch) return 80;
  if (isWordStartMatch) return 60;
  if (isContainsMatch) return 40;

  return 0;
};

export const convertDetailedListGroupsToQuickActionCommandGroups = (detailedListItemGroups: DetailedListItemGroup[]): QuickActionCommandGroup[] => {
  return detailedListItemGroups.map(detailedListItemGroup => ({
    commands: detailedListItemGroup.children?.map(detailedListItem => convertDetailedListItemToQuickActionCommand(detailedListItem)) as QuickActionCommand[],
    actions: detailedListItemGroup.actions,
    groupName: detailedListItemGroup.groupName,
    icon: detailedListItemGroup.icon
  }));
};

export const convertDetailedListItemToQuickActionCommand = (detailedListItem: DetailedListItem): QuickActionCommand => {
  return {
    command: detailedListItem.title ?? '',
    label: detailedListItem.name,
    ...(detailedListItem.children != null ? { children: convertDetailedListGroupsToQuickActionCommandGroups(detailedListItem.children) } : {}),
    description: detailedListItem.description,
    disabled: detailedListItem.disabled,
    icon: detailedListItem.icon,
    id: detailedListItem.id,
    placeholder: detailedListItem.followupText,
    route: detailedListItem.keywords,
  };
};

export const convertQuickActionCommandGroupsToDetailedListGroups = (quickActionCommandGroup: QuickActionCommandGroup[]): DetailedListItemGroup[] => {
  return quickActionCommandGroup.map(quickActionCommandGroup => ({
    children: quickActionCommandGroup.commands?.map(quickActionCommand => convertQuickActionCommandToDetailedListItem(quickActionCommand)),
    actions: quickActionCommandGroup.actions,
    groupName: quickActionCommandGroup.groupName,
    icon: quickActionCommandGroup.icon
  }));
};

export const convertQuickActionCommandToDetailedListItem = (quickActionCommand: QuickActionCommand): DetailedListItem => {
  return {
    title: quickActionCommand.command,
    name: quickActionCommand.label,
    followupText: quickActionCommand.placeholder,
    ...(quickActionCommand.children != null ? { children: convertQuickActionCommandGroupsToDetailedListGroups(quickActionCommand.children) } : {}),
    description: quickActionCommand.description,
    disabled: quickActionCommand.disabled,
    icon: quickActionCommand.icon,
    id: quickActionCommand.id,
    keywords: quickActionCommand.route,
    disabledText: quickActionCommand.disabledText
  };
};

export const chunkArray = <T>(array: T[], chunkSize: number): T[][] => {
  const result: T[][] = [];
  for (let i = 0; i < array.length; i += chunkSize) {
    result.push(array.slice(i, i + chunkSize));
  }
  return result;
};
