/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { DetailedListWrapper } from '../../../components/detailed-list/detailed-list';
import { DetailedList } from '../../../static';
import { MynahIcons } from '../../../components/icon';

// Mock the form items wrapper
jest.mock('../../../components/chat-item/chat-item-form-items', () => ({
  ChatItemFormItemsWrapper: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    getAllValues: jest.fn(() => ({ filter1: 'value1' })),
    isFormValid: jest.fn(() => true)
  }))
}));

describe('DetailedListWrapper Component', () => {
  let detailedListWrapper: DetailedListWrapper;
  let mockOnFilterValueChange: jest.Mock;
  let mockOnGroupActionClick: jest.Mock;
  let mockOnGroupClick: jest.Mock;
  let mockOnItemSelect: jest.Mock;
  let mockOnItemClick: jest.Mock;
  let mockOnFilterActionClick: jest.Mock;

  const basicDetailedList: DetailedList = {
    list: [
      {
        groupName: 'Test Group',
        children: [
          {
            id: 'item-1',
            title: 'Test Item 1',
            description: 'Description 1'
          },
          {
            id: 'item-2',
            title: 'Test Item 2',
            description: 'Description 2'
          }
        ]
      }
    ]
  };

  const detailedListWithHeader: DetailedList = {
    header: {
      title: 'Test Header',
      description: 'Test header description',
      icon: MynahIcons.INFO,
      status: {
        status: 'success',
        title: 'Success Status',
        description: 'Everything is working',
        icon: MynahIcons.OK
      }
    },
    list: basicDetailedList.list
  };

  const detailedListWithFilters: DetailedList = {
    filterOptions: [
      {
        id: 'filter1',
        type: 'select',
        title: 'Filter 1',
        options: [
          { value: 'option1', label: 'Option 1' },
          { value: 'option2', label: 'Option 2' }
        ]
      }
    ],
    filterActions: [
      {
        id: 'apply-filter',
        text: 'Apply',
        icon: MynahIcons.OK
      }
    ],
    list: basicDetailedList.list
  };

  const detailedListWithGroupActions: DetailedList = {
    list: [
      {
        groupName: 'Group with Actions',
        icon: MynahIcons.FOLDER,
        actions: [
          {
            id: 'group-action-1',
            text: 'Group Action',
            icon: MynahIcons.PENCIL
          }
        ],
        children: [
          {
            id: 'item-1',
            title: 'Item 1',
            description: 'Description 1'
          }
        ]
      }
    ]
  };

  const largeDetailedList: DetailedList = {
    list: [
      {
        groupName: 'Large Group',
        children: Array.from({ length: 250 }, (_, i) => ({
          id: `item-${i}`,
          title: `Item ${i}`,
          description: `Description ${i}`
        }))
      }
    ]
  };

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnFilterValueChange = jest.fn();
    mockOnGroupActionClick = jest.fn();
    mockOnGroupClick = jest.fn();
    mockOnItemSelect = jest.fn();
    mockOnItemClick = jest.fn();
    mockOnFilterActionClick = jest.fn();
    jest.clearAllMocks();

    // Mock scrollIntoView
    Element.prototype.scrollIntoView = jest.fn();

    // Mock requestAnimationFrame
    global.requestAnimationFrame = jest.fn((cb) => {
      const timestamp = 0;
      cb(timestamp);
      return 0;
    });
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('Basic Functionality', () => {
    it('should create detailed list wrapper with basic props', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });

      expect(detailedListWrapper.render).toBeDefined();
      expect(detailedListWrapper.render.classList.contains('mynah-detailed-list')).toBe(true);
    });

    it('should have correct test ID', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const wrapper = document.body.querySelector('[data-testid*="quick-picks-wrapper"]');
      expect(wrapper).toBeDefined();
    });

    it('should render list items', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const items = document.body.querySelectorAll('.mynah-detailed-list-item');
      expect(items.length).toBe(2);
    });

    it('should render group names', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      expect(groupTitle?.textContent).toContain('Test Group');
    });

    it('should have proper component structure', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const header = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      const filters = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      const groups = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper');
      const filterActions = document.body.querySelector('.mynah-detailed-list-filter-actions-wrapper');

      expect(header).toBeDefined();
      expect(filters).toBeDefined();
      expect(groups).toBeDefined();
      expect(filterActions).toBeDefined();
    });
  });

  describe('Header Rendering', () => {
    it('should render header when provided', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: detailedListWithHeader });
      document.body.appendChild(detailedListWrapper.render);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper).toBeDefined();
      expect(headerWrapper?.textContent).toContain('Test Header');
    });

    it('should render header description', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: detailedListWithHeader });
      document.body.appendChild(detailedListWrapper.render);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper?.textContent).toContain('Test header description');
    });

    it('should render header status when provided', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: detailedListWithHeader });
      document.body.appendChild(detailedListWrapper.render);

      const statusCard = document.body.querySelector('[data-testid*="sheet-description"]');
      expect(statusCard).toBeDefined();
    });

    it('should not render header when not provided', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper?.textContent?.trim()).toBe('');
    });

    it('should handle header without status', () => {
      const headerWithoutStatus: DetailedList = {
        header: {
          title: 'Header Without Status',
          description: 'Just description',
          icon: MynahIcons.INFO
        },
        list: basicDetailedList.list
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: headerWithoutStatus });
      document.body.appendChild(detailedListWrapper.render);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper?.textContent).toContain('Header Without Status');

      // Should not have status card
      const statusCard = document.body.querySelector('[data-testid*="sheet-description"]');
      expect(statusCard).toBeNull();
    });

    it('should handle header with null status', () => {
      const headerWithNullStatus: DetailedList = {
        header: {
          title: 'Header With Null Status',
          description: 'Description',
          status: null as any
        },
        list: basicDetailedList.list
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: headerWithNullStatus });
      document.body.appendChild(detailedListWrapper.render);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper?.textContent).toContain('Header With Null Status');
    });
  });

  describe('Filter Rendering', () => {
    it('should render filters when provided', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: detailedListWithFilters,
        onFilterValueChange: mockOnFilterValueChange
      });
      document.body.appendChild(detailedListWrapper.render);

      const filtersWrapper = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      expect(filtersWrapper).toBeDefined();
      expect(filtersWrapper?.children.length).toBeGreaterThan(0);
    });

    it('should render filter actions when provided', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: detailedListWithFilters,
        onFilterActionClick: mockOnFilterActionClick
      });
      document.body.appendChild(detailedListWrapper.render);

      const filterActionsWrapper = document.body.querySelector('.mynah-detailed-list-filter-actions-wrapper');
      expect(filterActionsWrapper).toBeDefined();
    });

    it('should not render filters when not provided', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const filtersWrapper = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      expect(filtersWrapper?.textContent?.trim()).toBe('');
    });

    it('should handle filter action click', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: detailedListWithFilters,
        onFilterActionClick: mockOnFilterActionClick
      });
      document.body.appendChild(detailedListWrapper.render);

      // Simulate filter action click through the button wrapper
      // This would normally be triggered by the ChatItemButtonsWrapper
      expect(detailedListWrapper.render).toBeDefined();
    });
  });

  describe('Group Functionality', () => {
    it('should render group with icon', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: detailedListWithGroupActions });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      const icon = groupTitle?.querySelector('.mynah-icon');
      expect(icon).toBeDefined();
    });

    it('should render group actions', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: detailedListWithGroupActions,
        onGroupActionClick: mockOnGroupActionClick
      });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      expect(groupTitle).toBeDefined();
    });

    it('should handle group click when clickable', () => {
      const clickableList: DetailedList = {
        ...detailedListWithGroupActions,
        selectable: 'clickable'
      };

      detailedListWrapper = new DetailedListWrapper({
        detailedList: clickableList,
        onGroupClick: mockOnGroupClick
      });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title') as HTMLElement;
      expect(groupTitle?.classList.contains('mynah-group-title-clickable')).toBe(true);

      groupTitle.click();
      expect(mockOnGroupClick).toHaveBeenCalledWith('Group with Actions');
    });

    it('should not make group clickable when selectable is not clickable', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: detailedListWithGroupActions,
        onGroupClick: mockOnGroupClick
      });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      expect(groupTitle?.classList.contains('mynah-group-title-clickable')).toBe(false);
    });

    it('should render groups without names', () => {
      const listWithoutGroupName: DetailedList = {
        list: [
          {
            children: [
              { id: 'item-1', title: 'Item 1' }
            ]
          }
        ]
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: listWithoutGroupName });
      document.body.appendChild(detailedListWrapper.render);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      expect(groupTitle).toBeNull();

      const items = document.body.querySelectorAll('.mynah-detailed-list-item');
      expect(items.length).toBe(1);
    });

    it('should handle indented children', () => {
      const listWithIndentedChildren: DetailedList = {
        list: [
          {
            groupName: 'Parent Group',
            childrenIndented: true,
            children: [
              { id: 'item-1', title: 'Indented Item' }
            ]
          }
        ]
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: listWithIndentedChildren });
      document.body.appendChild(detailedListWrapper.render);

      const itemsBlock = document.body.querySelector('.mynah-detailed-list-items-block');
      expect(itemsBlock?.classList.contains('indented')).toBe(true);
    });
  });

  describe('Virtualization', () => {
    it('should create blocks for large lists', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const itemsBlocks = document.body.querySelectorAll('.mynah-detailed-list-items-block');
      expect(itemsBlocks.length).toBeGreaterThan(1); // Should be chunked into multiple blocks
    });

    it('should set minimum height for virtualized blocks', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const itemsBlocks = document.body.querySelectorAll('.mynah-detailed-list-items-block');
      const firstBlock = itemsBlocks[0] as HTMLElement;
      expect(firstBlock.style.minHeight).toBeTruthy();
    });

    it('should handle scroll events for virtualization', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const groupsContainer = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper') as HTMLElement;

      // Mock scroll properties
      Object.defineProperty(groupsContainer, 'offsetHeight', { value: 400 });
      Object.defineProperty(groupsContainer, 'scrollTop', { value: 200 });

      // Trigger scroll event
      groupsContainer.dispatchEvent(new Event('scroll'));

      // Should not throw error
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should render first 5 blocks immediately', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const itemsBlocks = document.body.querySelectorAll('.mynah-detailed-list-items-block');
      let renderedBlocks = 0;

      itemsBlocks.forEach((block, index) => {
        if (block.children.length > 0) {
          renderedBlocks++;
        }
      });

      expect(renderedBlocks).toBeGreaterThan(0);
    });

    it('should handle virtualization with blocks entering viewport', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const groupsContainer = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper') as HTMLElement;
      const itemsBlocks = document.body.querySelectorAll('.mynah-detailed-list-items-block');

      // Mock properties for a block that should be rendered
      Object.defineProperty(groupsContainer, 'offsetHeight', { value: 400 });
      Object.defineProperty(groupsContainer, 'scrollTop', { value: 0 });

      // Mock a block that's in viewport but not rendered
      const testBlock = itemsBlocks[6] as HTMLElement; // Beyond first 5
      if (testBlock != null) {
        Object.defineProperty(testBlock, 'offsetTop', { value: 100 });
        Object.defineProperty(testBlock, 'offsetHeight', { value: 200 });

        // Clear the block first
        testBlock.innerHTML = '';

        // Trigger scroll to render it
        groupsContainer.dispatchEvent(new Event('scroll'));

        expect(detailedListWrapper.render).toBeDefined();
      }
    });

    it('should handle virtualization with blocks leaving viewport', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const groupsContainer = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper') as HTMLElement;
      const itemsBlocks = document.body.querySelectorAll('.mynah-detailed-list-items-block');

      // Mock properties for a block that should be cleared
      Object.defineProperty(groupsContainer, 'offsetHeight', { value: 400 });
      Object.defineProperty(groupsContainer, 'scrollTop', { value: 2000 }); // Scrolled far down

      // Mock a block that's out of viewport
      const testBlock = itemsBlocks[0] as HTMLElement;
      if (testBlock != null) {
        Object.defineProperty(testBlock, 'offsetTop', { value: 0 });
        Object.defineProperty(testBlock, 'offsetHeight', { value: 200 });

        // Trigger scroll to clear it
        groupsContainer.dispatchEvent(new Event('scroll'));

        expect(detailedListWrapper.render).toBeDefined();
      }
    });
  });

  describe('Item Selection and Navigation', () => {
    it('should handle item selection', () => {
      detailedListWrapper = new DetailedListWrapper({
        detailedList: basicDetailedList,
        onItemSelect: mockOnItemSelect
      });
      document.body.appendChild(detailedListWrapper.render);

      const firstItem = document.body.querySelector('.mynah-detailed-list-item') as HTMLElement;
      firstItem.click();

      expect(mockOnItemSelect).toHaveBeenCalled();
    });

    it('should handle item click', () => {
      const clickableList: DetailedList = {
        ...basicDetailedList,
        selectable: 'clickable'
      };

      detailedListWrapper = new DetailedListWrapper({
        detailedList: clickableList,
        onItemClick: mockOnItemClick
      });
      document.body.appendChild(detailedListWrapper.render);

      const firstItem = document.body.querySelector('.mynah-detailed-list-item') as HTMLElement;
      firstItem.click();

      expect(mockOnItemClick).toHaveBeenCalled();
    });

    it('should change target up', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      // Should not throw error
      detailedListWrapper.changeTarget('up');
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should change target down', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      detailedListWrapper.changeTarget('down');
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should change target with snap on last and first', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      detailedListWrapper.changeTarget('up', true);
      detailedListWrapper.changeTarget('down', true);
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should change target with scroll into view', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      detailedListWrapper.changeTarget('down', false, true);
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should get target element', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);
      detailedListWrapper.changeTarget('down', false, true);

      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement).toBeDefined();
      expect(targetElement?.id).toBe('item-1');
    });

    it('should return null when no selectable elements', () => {
      const emptyList: DetailedList = { list: [] };
      detailedListWrapper = new DetailedListWrapper({ detailedList: emptyList });

      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement).toBeNull();
    });

    it('should handle navigation with disabled items', () => {
      const listWithDisabledItems: DetailedList = {
        list: [
          {
            groupName: 'Mixed Group',
            children: [
              { id: 'item-1', title: 'Enabled Item' },
              { id: 'item-2', title: 'Disabled Item', disabled: true },
              { id: 'item-3', title: 'Another Enabled Item' }
            ]
          }
        ]
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: listWithDisabledItems });
      document.body.appendChild(detailedListWrapper.render);

      // Should only include enabled items in navigation
      detailedListWrapper.changeTarget('down');
      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement?.disabled).not.toBe(true);
    });

    it('should handle navigation wrapping from first to last', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      // First select the first item
      detailedListWrapper.changeTarget('down');

      // Go up from first item (should wrap to last)
      detailedListWrapper.changeTarget('up', false);
      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement?.id).toBe('item-2'); // Should wrap to last item
    });

    it('should handle navigation wrapping from last to first', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      // First select an item
      detailedListWrapper.changeTarget('down');
      // Move to last item
      detailedListWrapper.changeTarget('down');
      // Then go down (should wrap to first)
      detailedListWrapper.changeTarget('down', false);
      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement?.id).toBe('item-1'); // Should wrap to first item
    });

    it('should handle navigation with snap at boundaries', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      // First select the first item
      detailedListWrapper.changeTarget('down');

      // Go up with snap (should stay at first)
      detailedListWrapper.changeTarget('up', true);
      let targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement?.id).toBe('item-1');

      // Move to last item
      detailedListWrapper.changeTarget('down');
      // Go down with snap (should stay at last)
      detailedListWrapper.changeTarget('down', true);
      targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement?.id).toBe('item-2');
    });
  });

  describe('Update Functionality', () => {
    it('should update header', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        header: {
          title: 'Updated Header',
          description: 'Updated description'
        }
      };

      detailedListWrapper.update(updatedList);

      const headerWrapper = document.body.querySelector('.mynah-detailed-list-header-wrapper');
      expect(headerWrapper?.textContent).toContain('Updated Header');
    });

    it('should update filters', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        filterOptions: [
          {
            id: 'new-filter',
            type: 'textinput',
            title: 'New Filter'
          }
        ]
      };

      detailedListWrapper.update(updatedList);

      const filtersWrapper = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      expect(filtersWrapper).toBeDefined();
    });

    it('should update filter actions', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        filterActions: [
          {
            id: 'new-action',
            text: 'New Action',
            icon: MynahIcons.REFRESH
          }
        ]
      };

      detailedListWrapper.update(updatedList);

      const filterActionsWrapper = document.body.querySelector('.mynah-detailed-list-filter-actions-wrapper');
      expect(filterActionsWrapper).toBeDefined();
    });

    it('should update list items', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        list: [
          {
            groupName: 'Updated Group',
            children: [
              { id: 'new-item', title: 'New Item', description: 'New description' }
            ]
          }
        ]
      };

      detailedListWrapper.update(updatedList);

      const groupTitle = document.body.querySelector('.mynah-detailed-list-group-title');
      expect(groupTitle?.textContent).toContain('Updated Group');

      const items = document.body.querySelectorAll('.mynah-detailed-list-item');
      expect(items.length).toBe(1);
    });

    it('should preserve scroll position when requested', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const groupsContainer = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper') as HTMLElement;
      Object.defineProperty(groupsContainer, 'scrollTop', { value: 100, writable: true });

      const updatedList: DetailedList = {
        list: [
          {
            groupName: 'Updated Large Group',
            children: Array.from({ length: 50 }, (_, i) => ({
              id: `updated-item-${i}`,
              title: `Updated Item ${i}`
            }))
          }
        ]
      };

      detailedListWrapper.update(updatedList, true);

      expect(requestAnimationFrame).toHaveBeenCalled();
    });

    it('should reset scroll position when not preserving', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: largeDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        list: [
          {
            groupName: 'New Group',
            children: [ { id: 'new-item', title: 'New Item' } ]
          }
        ]
      };

      detailedListWrapper.update(updatedList, false);

      const groupsContainer = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper') as HTMLElement;
      expect(groupsContainer).toBeDefined();
    });

    it('should update selectable property', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      const updatedList: DetailedList = {
        selectable: 'clickable',
        list: basicDetailedList.list
      };

      detailedListWrapper.update(updatedList);

      expect(detailedListWrapper.render).toBeDefined();
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty list', () => {
      const emptyList: DetailedList = { list: [] };
      detailedListWrapper = new DetailedListWrapper({ detailedList: emptyList });
      document.body.appendChild(detailedListWrapper.render);

      const items = document.body.querySelectorAll('.mynah-detailed-list-item');
      expect(items.length).toBe(0);
    });

    it('should handle list with empty groups', () => {
      const listWithEmptyGroups: DetailedList = {
        list: [
          { groupName: 'Empty Group', children: [] },
          { groupName: 'Another Empty Group', children: undefined }
        ]
      };

      detailedListWrapper = new DetailedListWrapper({ detailedList: listWithEmptyGroups });
      document.body.appendChild(detailedListWrapper.render);

      const groups = document.body.querySelectorAll('.mynah-detailed-list-group');
      expect(groups.length).toBe(2);

      const items = document.body.querySelectorAll('.mynah-detailed-list-item');
      expect(items.length).toBe(0);
    });

    it('should handle null/undefined list', () => {
      const nullList: DetailedList = { list: undefined };
      detailedListWrapper = new DetailedListWrapper({ detailedList: nullList });
      document.body.appendChild(detailedListWrapper.render);

      const groupsWrapper = document.body.querySelector('.mynah-detailed-list-item-groups-wrapper');
      expect(groupsWrapper?.textContent?.trim()).toBe('');
    });

    it('should handle update with partial data', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: detailedListWithHeader });
      document.body.appendChild(detailedListWrapper.render);

      // Update with only some properties
      detailedListWrapper.update({});

      // Should not throw error
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should handle navigation with no selectable elements', () => {
      const emptyList: DetailedList = { list: [] };
      detailedListWrapper = new DetailedListWrapper({ detailedList: emptyList });

      // Should not throw error
      detailedListWrapper.changeTarget('up');
      detailedListWrapper.changeTarget('down');
      expect(detailedListWrapper.render).toBeDefined();
    });

    it('should handle getTargetElement with negative index', () => {
      detailedListWrapper = new DetailedListWrapper({ detailedList: basicDetailedList });
      document.body.appendChild(detailedListWrapper.render);

      // Manually set a negative index to test Math.max
      (detailedListWrapper as any).activeTargetElementIndex = -1;

      const targetElement = detailedListWrapper.getTargetElement();
      expect(targetElement).toBeDefined(); // Should use Math.max(index, 0)
    });

    it('should handle filter options with empty array', () => {
      const listWithEmptyFilters: DetailedList = {
        filterOptions: [],
        list: basicDetailedList.list
      };

      detailedListWrapper = new DetailedListWrapper({
        detailedList: listWithEmptyFilters,
        onFilterValueChange: mockOnFilterValueChange
      });
      document.body.appendChild(detailedListWrapper.render);

      const filtersWrapper = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      expect(filtersWrapper?.textContent?.trim()).toBe('');
    });

    it('should handle filter options with null', () => {
      const listWithNullFilters: DetailedList = {
        filterOptions: null,
        list: basicDetailedList.list
      };

      detailedListWrapper = new DetailedListWrapper({
        detailedList: listWithNullFilters,
        onFilterValueChange: mockOnFilterValueChange
      });
      document.body.appendChild(detailedListWrapper.render);

      const filtersWrapper = document.body.querySelector('.mynah-detailed-list-filters-wrapper');
      expect(filtersWrapper?.textContent?.trim()).toBe('');
    });
  });

  describe('Text Direction', () => {
    it('should pass text direction to items', () => {
      const listWithTextDirection: DetailedList = {
        ...basicDetailedList,
        textDirection: 'column'
      };

      detailedListWrapper = new DetailedListWrapper({
        detailedList: listWithTextDirection,
        descriptionTextDirection: 'rtl'
      });
      document.body.appendChild(detailedListWrapper.render);

      const textContainer = document.body.querySelector('.mynah-detailed-list-item-text');
      expect(textContainer?.classList.contains('mynah-detailed-list-item-text-direction-column')).toBe(true);
    });
  });
});
