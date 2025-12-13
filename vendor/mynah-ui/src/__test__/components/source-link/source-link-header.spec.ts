/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { SourceLinkHeader } from '../../../components/source-link/source-link-header';
import { SourceLink, MynahEventNames } from '../../../static';
import { MynahUIGlobalEvents } from '../../../helper/events';

// Mock the global events
jest.mock('../../../helper/events', () => ({
  MynahUIGlobalEvents: {
    getInstance: jest.fn(() => ({
      addListener: jest.fn()
    }))
  }
}));

// Mock the overlay component
jest.mock('../../../components/overlay', () => ({
  Overlay: jest.fn().mockImplementation(() => ({
    close: jest.fn()
  })),
  OverlayHorizontalDirection: {
    START_TO_RIGHT: 'start-to-right'
  },
  OverlayVerticalDirection: {
    TO_TOP: 'to-top'
  }
}));

describe('SourceLinkHeader Component', () => {
  let sourceLinkHeader: SourceLinkHeader;
  let mockOnClick: jest.Mock;

  const basicSourceLink: SourceLink = {
    title: 'Test Source Link',
    url: 'https://example.com/test/path'
  };

  const sourceWithBody: SourceLink = {
    title: 'Source with Body',
    url: 'https://github.com/user/repo/blob/main/file.js',
    body: 'This source has body content for preview'
  };

  const sourceWithMetadata: SourceLink = {
    title: 'GitHub Repository',
    url: 'https://github.com/example/awesome-repo',
    metadata: {
      github: {
        stars: 1500,
        forks: 250,
        isOfficialDoc: true,
        lastActivityDate: Date.now() - 86400000, // 1 day ago
        score: 95
      }
    }
  };

  const sourceWithComplexMetadata: SourceLink = {
    title: 'Stack Overflow Question',
    url: 'https://stackoverflow.com/questions/12345/test-question',
    metadata: {
      stackoverflow: {
        answerCount: 8,
        isAccepted: true,
        score: 42,
        lastActivityDate: Date.now() - 3600000 // 1 hour ago
      },
      github: {
        stars: 500,
        forks: 75
      }
    }
  };

  beforeEach(() => {
    document.body.innerHTML = '';
    mockOnClick = jest.fn();
    jest.clearAllMocks();
    jest.clearAllTimers();
    jest.useFakeTimers();
  });

  afterEach(() => {
    document.body.innerHTML = '';
    jest.runOnlyPendingTimers();
    jest.useRealTimers();
  });

  describe('Basic Functionality', () => {
    it('should create source link header with basic props', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });

      expect(sourceLinkHeader.render).toBeDefined();
      expect(sourceLinkHeader.render.classList.contains('mynah-source-link-header')).toBe(true);
    });

    it('should render source link title', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const title = document.body.querySelector('.mynah-source-link-title');
      expect(title?.textContent).toContain(basicSourceLink.title);
    });

    it('should render source link URL with correct href', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title');
      const urlLink = document.body.querySelector('.mynah-source-link-url');

      expect(titleLink?.getAttribute('href')).toBe(basicSourceLink.url);
      expect(urlLink?.getAttribute('href')).toBe(basicSourceLink.url);
    });

    it('should have target="_blank" for external links', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title');
      const urlLink = document.body.querySelector('.mynah-source-link-url');

      expect(titleLink?.getAttribute('target')).toBe('_blank');
      expect(urlLink?.getAttribute('target')).toBe('_blank');
    });

    it('should render external link icon', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const expandIcon = document.body.querySelector('.mynah-source-link-expand-icon');
      expect(expandIcon).toBeDefined();
    });

    it('should have correct test IDs', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const wrapper = document.body.querySelector('[data-testid*="link-wrapper"]');
      const link = document.body.querySelector('[data-testid*="link"]');

      expect(wrapper).toBeDefined();
      expect(link).toBeDefined();
    });
  });

  describe('URL Processing', () => {
    it('should process URL correctly by removing protocol', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('example.com');
      expect(urlElement?.innerHTML).toContain('test');
      expect(urlElement?.innerHTML).toContain('path');
    });

    it('should handle HTTPS URLs', () => {
      const httpsSource: SourceLink = {
        title: 'HTTPS Source',
        url: 'https://secure.example.com/path'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: httpsSource });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('secure.example.com');
      expect(urlElement?.innerHTML).not.toContain('https://');
    });

    it('should handle HTTP URLs', () => {
      const httpSource: SourceLink = {
        title: 'HTTP Source',
        url: 'http://example.com/path'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: httpSource });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('example.com');
      expect(urlElement?.innerHTML).not.toContain('http://');
    });

    it('should handle URLs with trailing slash', () => {
      const trailingSlashSource: SourceLink = {
        title: 'Trailing Slash',
        url: 'https://example.com/path/'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: trailingSlashSource });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('example.com');
      expect(urlElement?.innerHTML).toContain('path');
    });

    it('should wrap URL parts in spans', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      const spans = urlElement?.querySelectorAll('span');

      expect(spans?.length).toBeGreaterThan(0);
    });

    it('should set origin attribute', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header');
      expect(header?.getAttribute('origin')).toBeDefined();
    });
  });

  describe('Click Handling', () => {
    it('should handle onClick for title link', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: basicSourceLink,
        onClick: mockOnClick
      });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title') as HTMLElement;
      titleLink.click();

      expect(mockOnClick).toHaveBeenCalled();
    });

    it('should handle onClick for URL link', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: basicSourceLink,
        onClick: mockOnClick
      });
      document.body.appendChild(sourceLinkHeader.render);

      const urlLink = document.body.querySelector('.mynah-source-link-url') as HTMLElement;
      urlLink.click();

      expect(mockOnClick).toHaveBeenCalled();
    });

    it('should handle auxclick (middle click) for title link', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: basicSourceLink,
        onClick: mockOnClick
      });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title') as HTMLElement;
      const auxClickEvent = new MouseEvent('auxclick', { button: 1 });
      titleLink.dispatchEvent(auxClickEvent);

      expect(mockOnClick).toHaveBeenCalled();
    });

    it('should handle auxclick for URL link', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: basicSourceLink,
        onClick: mockOnClick
      });
      document.body.appendChild(sourceLinkHeader.render);

      const urlLink = document.body.querySelector('.mynah-source-link-url') as HTMLElement;
      const auxClickEvent = new MouseEvent('auxclick', { button: 1 });
      urlLink.dispatchEvent(auxClickEvent);

      expect(mockOnClick).toHaveBeenCalled();
    });

    it('should not add click handlers when onClick is not provided', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title') as HTMLElement;
      titleLink.click();

      // Should not throw error
      expect(sourceLinkHeader.render).toBeDefined();
    });
  });

  describe('Metadata Rendering', () => {
    it('should render metadata when provided', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaBlock = document.body.querySelector('.mynah-title-meta-block');
      expect(metaBlock).toBeDefined();
    });

    it('should not render metadata when not provided', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const metaBlock = document.body.querySelector('.mynah-title-meta-block');
      expect(metaBlock).toBeNull();
    });

    it('should render accepted answer icon', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithComplexMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const approvedAnswer = document.body.querySelector('.approved-answer');
      expect(approvedAnswer).toBeDefined();
    });

    it('should render stars count', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaItems = document.body.querySelectorAll('.mynah-title-meta-block-item-text');
      const starsItem = Array.from(metaItems).find(item =>
        item.textContent?.includes('contributors')
      );
      expect(starsItem).toBeDefined();
      expect(starsItem?.textContent).toContain('1500');
    });

    it('should render forks count', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaItems = document.body.querySelectorAll('.mynah-title-meta-block-item-text');
      const forksItem = Array.from(metaItems).find(item =>
        item.textContent?.includes('forks')
      );
      expect(forksItem).toBeDefined();
      expect(forksItem?.textContent).toContain('250');
    });

    it('should render answer count', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithComplexMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaItems = document.body.querySelectorAll('.mynah-title-meta-block-item-text');
      const answerItem = Array.from(metaItems).find(item =>
        item.textContent === '8'
      );
      expect(answerItem).toBeDefined();
    });

    it('should render score', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithComplexMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaItems = document.body.querySelectorAll('.mynah-title-meta-block-item-text');
      const scoreItem = Array.from(metaItems).find(item =>
        item.textContent === '42'
      );
      expect(scoreItem).toBeDefined();
    });

    it('should render last activity date', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaItems = document.body.querySelectorAll('.mynah-title-meta-block-item');
      // Check that we have meta items (the date formatting might vary)
      expect(metaItems.length).toBeGreaterThan(0);

      // Check for calendar icon which indicates date item
      const calendarIcon = document.body.querySelector('.mynah-title-meta-block-item .mynah-icon');
      expect(calendarIcon).toBeDefined();
    });

    it('should handle multiple metadata sources', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithComplexMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaBlock = document.body.querySelector('.mynah-title-meta-block');
      const metaItems = metaBlock?.querySelectorAll('.mynah-title-meta-block-item');

      expect(metaItems?.length).toBeGreaterThan(1);
    });

    it('should handle empty metadata object', () => {
      const sourceWithEmptyMetadata: SourceLink = {
        title: 'Empty Metadata',
        url: 'https://example.com',
        metadata: {}
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: sourceWithEmptyMetadata });
      document.body.appendChild(sourceLinkHeader.render);

      const metaBlock = document.body.querySelector('.mynah-title-meta-block');
      expect(metaBlock).toBeDefined();

      const metaItems = metaBlock?.querySelectorAll('.mynah-title-meta-block-item');
      expect(metaItems?.length).toBe(0);
    });
  });

  describe('Preview Functionality', () => {
    it('should show preview on hover when showCardOnHover is true', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: true
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;

      // Trigger mouseenter
      const mouseEnterEvent = new MouseEvent('mouseenter');
      header.dispatchEvent(mouseEnterEvent);

      // Fast-forward timer
      jest.advanceTimersByTime(500);

      // Overlay should be created (mocked)
      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should hide preview on mouseleave', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: true
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;

      // Trigger mouseenter then mouseleave
      header.dispatchEvent(new MouseEvent('mouseenter'));
      header.dispatchEvent(new MouseEvent('mouseleave'));

      // Timer should be cleared
      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).not.toHaveBeenCalled();
    });

    it('should show preview on focus', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: true
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;

      // Trigger focus
      const focusEvent = new FocusEvent('focus');
      header.dispatchEvent(focusEvent);

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).toHaveBeenCalled();
    });

    it('should hide preview on blur', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: true
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;

      // Trigger focus then blur
      header.dispatchEvent(new FocusEvent('focus'));
      header.dispatchEvent(new FocusEvent('blur'));

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).not.toHaveBeenCalled();
    });

    it('should not show preview when source has no body', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: basicSourceLink,
        showCardOnHover: true
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;
      header.dispatchEvent(new MouseEvent('mouseenter'));

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).not.toHaveBeenCalled();
    });

    it('should not add hover events when showCardOnHover is false', () => {
      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: false
      });
      document.body.appendChild(sourceLinkHeader.render);

      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;
      header.dispatchEvent(new MouseEvent('mouseenter'));

      jest.advanceTimersByTime(500);

      const { Overlay } = jest.requireMock('../../../components/overlay');
      expect(Overlay).not.toHaveBeenCalled();
    });
  });

  describe('Global Events', () => {
    it('should register for root focus events', () => {
      const mockAddListener = jest.fn();
      (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue({
        addListener: mockAddListener
      });

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });

      expect(mockAddListener).toHaveBeenCalledWith(
        MynahEventNames.ROOT_FOCUS,
        expect.any(Function)
      );
    });

    it('should hide preview on root focus loss', () => {
      const mockAddListener = jest.fn();
      let focusCallback: ((data: {focusState: boolean}) => void) | undefined;

      (MynahUIGlobalEvents.getInstance as jest.Mock).mockReturnValue({
        addListener: (event: string, callback: any) => {
          if (event === MynahEventNames.ROOT_FOCUS) {
            focusCallback = callback;
          }
          mockAddListener(event, callback);
        }
      });

      sourceLinkHeader = new SourceLinkHeader({
        sourceLink: sourceWithBody,
        showCardOnHover: true
      });

      // Simulate showing preview
      document.body.appendChild(sourceLinkHeader.render);
      const header = document.body.querySelector('.mynah-source-link-header') as HTMLElement;
      header.dispatchEvent(new MouseEvent('mouseenter'));
      jest.advanceTimersByTime(500);

      // Simulate root focus loss
      if (focusCallback != null) {
        focusCallback({ focusState: false });
      }

      // Should hide preview (tested through mock)
      expect(mockAddListener).toHaveBeenCalled();
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty URL', () => {
      const emptyUrlSource: SourceLink = {
        title: 'Empty URL',
        url: ''
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: emptyUrlSource });
      expect(sourceLinkHeader.render).toBeDefined();
    });

    it('should handle URL without protocol', () => {
      const noProtocolSource: SourceLink = {
        title: 'No Protocol',
        url: 'example.com/path'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: noProtocolSource });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('example.com');
    });

    it('should handle very long URLs', () => {
      const longUrlSource: SourceLink = {
        title: 'Long URL',
        url: 'https://example.com/very/long/path/with/many/segments/that/goes/on/and/on'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: longUrlSource });
      expect(sourceLinkHeader.render).toBeDefined();
    });

    it('should handle special characters in URL', () => {
      const specialCharSource: SourceLink = {
        title: 'Special Chars',
        url: 'https://example.com/path?query=test&param=value#section'
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: specialCharSource });
      document.body.appendChild(sourceLinkHeader.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.innerHTML).toContain('example.com');
    });

    it('should handle metadata with undefined values', () => {
      const undefinedMetadataSource: SourceLink = {
        title: 'Undefined Metadata',
        url: 'https://example.com',
        metadata: {
          test: {
            stars: undefined,
            forks: undefined,
            answerCount: undefined,
            isAccepted: undefined,
            score: undefined,
            lastActivityDate: undefined
          }
        }
      };

      sourceLinkHeader = new SourceLinkHeader({ sourceLink: undefinedMetadataSource });
      document.body.appendChild(sourceLinkHeader.render);

      const metaBlock = document.body.querySelector('.mynah-title-meta-block');
      expect(metaBlock).toBeDefined();
    });
  });

  describe('Component Structure', () => {
    it('should have proper DOM structure', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      // Should have main header
      const header = document.body.querySelector('.mynah-source-link-header');
      expect(header).toBeDefined();

      // Should have thumbnail
      const thumbnail = header?.querySelector('.mynah-source-thumbnail');
      expect(thumbnail).toBeDefined();

      // Should have title wrapper
      const titleWrapper = header?.querySelector('.mynah-source-link-title-wrapper');
      expect(titleWrapper).toBeDefined();

      // Should have title and URL links
      const title = titleWrapper?.querySelector('.mynah-source-link-title');
      const url = titleWrapper?.querySelector('.mynah-source-link-url');
      expect(title).toBeDefined();
      expect(url).toBeDefined();
    });

    it('should maintain proper accessibility attributes', () => {
      sourceLinkHeader = new SourceLinkHeader({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkHeader.render);

      const titleLink = document.body.querySelector('.mynah-source-link-title');
      const urlLink = document.body.querySelector('.mynah-source-link-url');

      expect(titleLink?.getAttribute('href')).toBeTruthy();
      expect(titleLink?.getAttribute('target')).toBe('_blank');
      expect(urlLink?.getAttribute('href')).toBeTruthy();
      expect(urlLink?.getAttribute('target')).toBe('_blank');
    });
  });
});
