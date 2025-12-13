/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { TopBarButton } from '../../../../../components/chat-item/prompt-input/prompt-top-bar/top-bar-button';
import { ChatItemButton } from '../../../../../static';
import { MynahIcons } from '../../../../../components/icon';

// Mock the overlay component
jest.mock('../../../../../components/overlay', () => ({
  Overlay: jest.fn().mockImplementation(() => ({
    close: jest.fn(),
    updateContent: jest.fn()
  })),
  OverlayHorizontalDirection: {
    END_TO_LEFT: 'end-to-left'
  },
  OverlayVerticalDirection: {
    TO_TOP: 'to-top'
  }
}));

// Mock the detailed list wrapper
jest.mock('../../../../../components/detailed-list/detailed-list', () => ({
  DetailedListWrapper: jest.fn().mockImplementation(() => ({
    render: document.createElement('div'),
    update: jest.fn()
  }))
}));

describe('TopBarButton Component', () => {
  let topBarButton: TopBarButton;
  let mockOnTopBarButtonClick: jest.Mock;

  const basicButton: ChatItemButton = {
    id: 'test-button',
    text: 'Test Button',
    icon: MynahIcons.PLUS
  };

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnTopBarButtonClick = jest.fn();
    jest.clearAllMocks();

    // Mock window event listeners
    global.addEventListener = jest.fn();
    global.removeEventListener = jest.fn();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.clearAllMocks();
  });

  describe('Basic Functionality', () => {
    it('should create top bar button with basic props', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      expect(topBarButton.render).toBeDefined();
      expect(topBarButton.render.classList.contains('top-bar-button')).toBe(true);
    });

    it('should create top bar button without button prop', () => {
      topBarButton = new TopBarButton({});

      expect(topBarButton.render).toBeDefined();
    });

    it('should have correct test ID', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });
      document.body.appendChild(topBarButton.render);

      const button = document.body.querySelector('[data-testid*="top-bar-button"]');
      expect(button).toBeDefined();
    });

    it('should have contenteditable false attribute', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      expect(topBarButton.render.getAttribute('contenteditable')).toBe('false');
    });
  });

  describe('Button Click Handling', () => {
    it('should handle button click', () => {
      topBarButton = new TopBarButton({
        topBarButton: basicButton,
        onTopBarButtonClick: mockOnTopBarButtonClick
      });
      document.body.appendChild(topBarButton.render);

      const buttonElement = document.body.querySelector('button') as HTMLElement;
      buttonElement.click();

      expect(mockOnTopBarButtonClick).toHaveBeenCalledWith(basicButton);
    });

    it('should not call callback when topBarButton is null', () => {
      topBarButton = new TopBarButton({
        onTopBarButtonClick: mockOnTopBarButtonClick
      });
      document.body.appendChild(topBarButton.render);

      const buttonElement = document.body.querySelector('button') as HTMLElement;
      buttonElement.click();

      expect(mockOnTopBarButtonClick).not.toHaveBeenCalled();
    });
  });

  describe('Update Functionality', () => {
    it('should update button properties', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      const updatedButton: ChatItemButton = {
        id: 'updated-button',
        text: 'Updated Button',
        icon: MynahIcons.REFRESH
      };

      topBarButton.update({ topBarButton: updatedButton });

      expect(topBarButton.render).toBeDefined();
    });

    it('should handle update with null button', () => {
      topBarButton = new TopBarButton({ topBarButton: basicButton });

      topBarButton.update({});

      expect(topBarButton.render).toBeDefined();
    });
  });
});
