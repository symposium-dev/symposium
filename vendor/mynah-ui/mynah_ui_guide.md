# MynahUI GUI Capabilities Guide

## Overview

MynahUI is a data and event-driven chat interface library for browsers and webviews. This guide focuses on the interactive GUI capabilities relevant for building tool permission and approval workflows.

## Core Concepts

### Chat Items

Chat items are the fundamental building blocks of the conversation UI. Each chat item is a "card" that can contain various interactive elements.

**Basic Structure:**
```typescript
interface ChatItem {
  type: ChatItemType;           // Determines positioning and styling
  messageId?: string;            // Unique identifier for updates
  body?: string;                 // Markdown content
  buttons?: ChatItemButton[];    // Action buttons
  formItems?: ChatItemFormItem[]; // Form inputs
  fileList?: FileList;           // File tree display
  followUp?: FollowUpOptions;    // Quick action pills
  // ... many more options
}
```

**Chat Item Types:**
- `ANSWER` / `ANSWER_STREAM` / `CODE_RESULT` → Left-aligned (AI responses)
- `PROMPT` / `SYSTEM_PROMPT` → Right-aligned (user messages)
- `DIRECTIVE` → Transparent, no background

## Interactive Components

### 1. Buttons (`ChatItemButton`)

Buttons are the primary action mechanism for user approval/denial workflows.

**Interface:**
```typescript
interface ChatItemButton {
  id: string;                    // Unique identifier for the button
  text?: string;                 // Button label
  icon?: MynahIcons;             // Optional icon
  status?: 'main' | 'primary' | 'clear' | 'dimmed-clear' | 'info' | 'success' | 'warning' | 'error';
  keepCardAfterClick?: boolean;  // If false, removes card after click
  waitMandatoryFormItems?: boolean; // Disables until mandatory form items are filled
  disabled?: boolean;
  description?: string;          // Tooltip text
}
```

**Status Colors:**
- `main` - Primary brand color
- `primary` - Accent color
- `success` - Green (for approval actions)
- `error` - Red (for denial/rejection actions)
- `warning` - Yellow/orange
- `info` - Blue
- `clear` - Transparent background

**Event Handler:**
```typescript
onInBodyButtonClicked: (tabId: string, messageId: string, action: {
  id: string;
  text?: string;
  // ... other button properties
}) => void
```

**Example - Approval Buttons:**
```typescript
{
  type: ChatItemType.ANSWER,
  messageId: 'tool-approval-123',
  body: 'Tool execution request...',
  buttons: [
    {
      id: 'approve-once',
      text: 'Approve',
      status: 'primary',
      icon: MynahIcons.OK
    },
    {
      id: 'approve-session',
      text: 'Approve for Session',
      status: 'success',
      icon: MynahIcons.OK_CIRCLED
    },
    {
      id: 'deny',
      text: 'Deny',
      status: 'error',
      icon: MynahIcons.CANCEL,
      keepCardAfterClick: false  // Card disappears on denial
    }
  ]
}
```

### 2. Form Items (`ChatItemFormItem`)

Form items allow collecting structured user input alongside button actions.

**Available Form Types:**
- `textinput` / `textarea` / `numericinput` / `email`
- `select` (dropdown)
- `radiogroup` / `toggle`
- `checkbox` / `switch`
- `stars` (rating)
- `list` (dynamic list of items)
- `pillbox` (tag/pill input)

**Common Properties:**
```typescript
interface BaseFormItem {
  id: string;                // Unique identifier
  type: string;              // Form type
  mandatory?: boolean;       // Required field
  title?: string;            // Label
  description?: string;      // Help text
  tooltip?: string;          // Tooltip
  value?: string;            // Initial/current value
  disabled?: boolean;
}
```

**Example - Checkbox for "Remember Choice":**
```typescript
formItems: [
  {
    type: 'checkbox',
    id: 'remember-approval',
    label: 'Remember this choice for similar requests',
    value: 'false',
    tooltip: 'If checked, future requests for this tool will be automatically approved'
  }
]
```

**Example - Toggle for Options:**
```typescript
formItems: [
  {
    type: 'toggle',
    id: 'approval-scope',
    title: 'Approval Scope',
    value: 'once',
    options: [
      { value: 'once', label: 'Once', icon: MynahIcons.CHECK },
      { value: 'session', label: 'Session', icon: MynahIcons.STACK },
      { value: 'always', label: 'Always', icon: MynahIcons.OK_CIRCLED }
    ]
  }
]
```

**Event Handlers:**
```typescript
onFormChange: (tabId: string, messageId: string, item: ChatItemFormItem, value: any) => void
```

### 3. Content Display Options

#### Markdown Body

The `body` field supports full markdown including:
- Headings (`#`, `##`, `###`)
- Code blocks with syntax highlighting
- Inline code
- Links
- Lists (ordered/unordered)
- Blockquotes
- Tables

**Example - Displaying Tool Parameters:**
```typescript
body: `### Tool Execution Request

**Tool:** \`read_file\`

**Parameters:**
\`\`\`json
{
  "file_path": "/Users/niko/src/config.ts",
  "offset": 0,
  "limit": 100
}
\`\`\`

Do you want to allow this tool to execute?`
```

#### Custom Renderer

For complex layouts beyond markdown, use `customRenderer` with HTML markup:

```typescript
customRenderer: `
<div>
  <h4>Tool: <code>read_file</code></h4>
  <table>
    <tr>
      <th>Parameter</th>
      <th>Value</th>
    </tr>
    <tr>
      <td>file_path</td>
      <td><code>/Users/niko/src/config.ts</code></td>
    </tr>
    <tr>
      <td>offset</td>
      <td><code>0</code></td>
    </tr>
  </table>
</div>
`
```

#### Information Cards

For hierarchical content with status indicators:

```typescript
informationCard: {
  title: 'Security Notice',
  status: {
    status: 'warning',
    icon: MynahIcons.WARNING,
    body: 'This tool will access filesystem resources'
  },
  description: 'Review the parameters carefully',
  content: {
    body: '... detailed information ...'
  }
}
```

### 4. File Lists

Display file paths with actions and metadata:

```typescript
fileList: {
  fileTreeTitle: 'Files to be accessed',
  filePaths: ['/src/config.ts', '/src/main.ts'],
  details: {
    '/src/config.ts': {
      icon: MynahIcons.FILE,
      description: 'Configuration file',
      clickable: true
    }
  },
  actions: {
    '/src/config.ts': [
      {
        name: 'view-details',
        icon: MynahIcons.EYE,
        description: 'View file details'
      }
    ]
  }
}
```

**Event Handler:**
```typescript
onFileActionClick: (tabId: string, messageId: string, filePath: string, actionName: string) => void
```

### 5. Follow-Up Pills

Quick action buttons displayed as pills:

```typescript
followUp: {
  text: 'Quick actions',
  options: [
    {
      pillText: 'Approve All',
      icon: MynahIcons.OK,
      status: 'success',
      prompt: 'approve-all'  // Can trigger automatic actions
    },
    {
      pillText: 'Deny All',
      icon: MynahIcons.CANCEL,
      status: 'error',
      prompt: 'deny-all'
    }
  ]
}
```

**Event Handler:**
```typescript
onFollowUpClicked: (tabId: string, messageId: string, followUp: ChatItemAction) => void
```

## Card Behavior Options

### Visual States

```typescript
{
  status?: 'info' | 'success' | 'warning' | 'error';  // Colors the card border/icon
  shimmer?: boolean;         // Loading animation
  canBeVoted?: boolean;      // Show thumbs up/down
  canBeDismissed?: boolean;  // Show dismiss button
  snapToTop?: boolean;       // Pin to top of chat
  border?: boolean;          // Show border
  hoverEffect?: boolean;     // Highlight on hover
}
```

### Layout Options

```typescript
{
  fullWidth?: boolean;               // Stretch to container width
  padding?: boolean;                 // Internal padding
  contentHorizontalAlignment?: 'default' | 'center';
}
```

### Card Lifecycle

```typescript
{
  keepCardAfterClick?: boolean;      // On buttons - remove card after click
  autoCollapse?: boolean;            // Auto-collapse long content
}
```

## Updating Chat Items

Chat items can be updated after creation:

```typescript
// Add new chat item
mynahUI.addChatItem(tabId, chatItem);

// Update by message ID
mynahUI.updateChatAnswerWithMessageId(tabId, messageId, updatedChatItem);

// Update last streaming answer
mynahUI.updateLastChatAnswer(tabId, partialChatItem);
```

## Complete Example: Tool Approval Workflow

```typescript
// 1. Show tool approval request
mynahUI.addChatItem('main-tab', {
  type: ChatItemType.ANSWER,
  messageId: 'tool-approval-read-file-001',
  status: 'warning',
  icon: MynahIcons.LOCK,
  body: `### Tool Execution Request

**Tool:** \`read_file\`

**Description:** Read file contents from the filesystem

**Parameters:**
\`\`\`json
{
  "file_path": "/Users/nikomat/dev/mynah-ui/src/config.ts",
  "offset": 0,
  "limit": 2000
}
\`\`\`

**Security:** This tool will access local filesystem resources.`,
  
  formItems: [
    {
      type: 'checkbox',
      id: 'remember-read-file',
      label: 'Trust this tool for the remainder of the session',
      value: 'false'
    }
  ],
  
  buttons: [
    {
      id: 'approve',
      text: 'Approve',
      status: 'success',
      icon: MynahIcons.OK,
      keepCardAfterClick: false
    },
    {
      id: 'deny',
      text: 'Deny',
      status: 'error',
      icon: MynahIcons.CANCEL,
      keepCardAfterClick: false
    },
    {
      id: 'details',
      text: 'More Details',
      status: 'clear',
      icon: MynahIcons.INFO
    }
  ]
});

// 2. Handle button clicks
mynahUI.onInBodyButtonClicked = (tabId, messageId, action) => {
  if (messageId === 'tool-approval-read-file-001') {
    const formState = mynahUI.getFormState(tabId, messageId);
    const rememberChoice = formState['remember-read-file'] === 'true';
    
    switch (action.id) {
      case 'approve':
        // Execute tool
        // If rememberChoice, add to session whitelist
        break;
      case 'deny':
        // Cancel tool execution
        break;
      case 'details':
        // Show additional information
        mynahUI.updateChatAnswerWithMessageId(tabId, messageId, {
          informationCard: {
            title: 'Tool Details',
            content: {
              body: 'Detailed tool documentation...'
            }
          }
        });
        break;
    }
  }
};
```

## Progressive Updates

For multi-step approval flows, you can progressively update the same card:

```typescript
// Initial request
mynahUI.addChatItem(tabId, {
  messageId: 'approval-001',
  type: ChatItemType.ANSWER,
  body: 'Waiting for approval...',
  shimmer: true
});

// User approves
mynahUI.updateChatAnswerWithMessageId(tabId, 'approval-001', {
  body: 'Approved! Executing tool...',
  shimmer: true,
  buttons: []  // Remove buttons
});

// Execution complete
mynahUI.updateChatAnswerWithMessageId(tabId, 'approval-001', {
  body: 'Tool execution complete!',
  shimmer: false,
  status: 'success',
  icon: MynahIcons.OK_CIRCLED
});
```

## Sticky Cards

For persistent approval requests that stay above the prompt:

```typescript
mynahUI.updateStore(tabId, {
  promptInputStickyCard: {
    messageId: 'persistent-approval',
    body: 'Multiple tools are waiting for approval',
    status: 'warning',
    icon: MynahIcons.WARNING,
    buttons: [
      {
        id: 'review-pending',
        text: 'Review Pending',
        status: 'info'
      }
    ]
  }
});

// Clear sticky card
mynahUI.updateStore(tabId, {
  promptInputStickyCard: null
});
```

## Best Practices for Tool Approval UI

1. **Clear Tool Identity**: Always show tool name prominently
2. **Parameter Visibility**: Display all parameters the tool will receive
3. **Security Context**: Indicate security implications (file access, network, etc.)
4. **Action Clarity**: Use clear "Approve" vs "Deny" with appropriate status colors
5. **Scope Options**: Provide "once", "session", "always" choices when appropriate
6. **Non-blocking**: Use `keepCardAfterClick: false` to auto-dismiss after approval
7. **Progressive Disclosure**: Start simple, show details on demand
8. **Feedback**: Update card state to show execution progress after approval

## Key Event Handlers

```typescript
interface MynahUIProps {
  onInBodyButtonClicked?: (tabId: string, messageId: string, action: ChatItemButton) => void;
  onFollowUpClicked?: (tabId: string, messageId: string, followUp: ChatItemAction) => void;
  onFormChange?: (tabId: string, messageId: string, item: ChatItemFormItem, value: any) => void;
  onFileActionClick?: (tabId: string, messageId: string, filePath: string, actionName: string) => void;
  // ... many more
}
```

## Reference

- Full documentation: [mynah-ui/docs/DATAMODEL.md](./docs/DATAMODEL.md)
- Type definitions: [mynah-ui/src/static.ts](./src/static.ts)
- Examples: [mynah-ui/example/src/samples/](./example/src/samples/)
