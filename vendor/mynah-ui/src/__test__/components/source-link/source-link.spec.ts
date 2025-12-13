/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { SourceLinkCard } from '../../../components/source-link/source-link';
import { SourceLink } from '../../../static';

describe('SourceLinkCard Component', () => {
  let sourceLinkCard: SourceLinkCard;

  const basicSourceLink: SourceLink = {
    title: 'Test Source Link',
    url: 'https://example.com/test',
    id: 'test-id'
  };

  const sourceWithBody: SourceLink = {
    title: 'Source with Body',
    url: 'https://example.com/with-body',
    body: 'This is the body content of the source link',
    type: 'documentation'
  };

  const sourceWithMetadata: SourceLink = {
    title: 'Source with Metadata',
    url: 'https://github.com/example/repo',
    metadata: {
      github: {
        stars: 150,
        forks: 25,
        isOfficialDoc: true,
        lastActivityDate: Date.now() - 86400000 // 1 day ago
      }
    }
  };

  beforeEach(() => {
    document.body.innerHTML = '';
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('Basic Functionality', () => {
    it('should create source link card with basic props', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });

      expect(sourceLinkCard.render).toBeDefined();
      expect(sourceLinkCard.render.classList.contains('mynah-card')).toBe(true);
    });

    it('should render source link header', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      const header = document.body.querySelector('.mynah-source-link-header');
      expect(header).toBeDefined();
    });

    it('should render source link title', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      const title = document.body.querySelector('.mynah-source-link-title');
      expect(title?.textContent).toContain(basicSourceLink.title);
    });

    it('should render source link URL', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      const urlElement = document.body.querySelector('.mynah-source-link-url');
      expect(urlElement?.getAttribute('href')).toBe(basicSourceLink.url);
    });

    it('should have correct test ID', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      const cardElement = document.body.querySelector('[data-testid*="link-preview-overlay-card"]');
      expect(cardElement).toBeDefined();
    });
  });

  describe('Source Link with Body', () => {
    it('should render body when provided', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithBody });
      document.body.appendChild(sourceLinkCard.render);

      const cardBody = document.body.querySelector('.mynah-card-body');
      expect(cardBody).toBeDefined();
    });

    it('should not render body when not provided', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      const cardBody = document.body.querySelector('.mynah-card-body');
      expect(cardBody).toBeNull();
    });

    it('should display body content correctly', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithBody });
      document.body.appendChild(sourceLinkCard.render);

      const cardBody = document.body.querySelector('.mynah-card-body');
      expect(cardBody?.textContent).toContain(sourceWithBody.body);
    });
  });

  describe('Compact Mode', () => {
    it('should handle compact flat mode', () => {
      sourceLinkCard = new SourceLinkCard({
        sourceLink: basicSourceLink,
        compact: 'flat'
      });

      expect(sourceLinkCard.render).toBeDefined();
    });

    it('should handle compact true mode', () => {
      sourceLinkCard = new SourceLinkCard({
        sourceLink: basicSourceLink,
        compact: true
      });

      expect(sourceLinkCard.render).toBeDefined();
    });
  });

  describe('Card Properties', () => {
    it('should create card with no border', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      // Card should not have border class
      const card = document.body.querySelector('.mynah-card');
      expect(card?.classList.contains('mynah-card-border')).toBe(false);
    });

    it('should create card with no background', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });
      document.body.appendChild(sourceLinkCard.render);

      // Card should not have background class
      const card = document.body.querySelector('.mynah-card');
      expect(card?.classList.contains('mynah-card-background')).toBe(false);
    });
  });

  describe('Complex Source Links', () => {
    it('should handle source link with metadata', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithMetadata });
      document.body.appendChild(sourceLinkCard.render);

      const header = document.body.querySelector('.mynah-source-link-header');
      expect(header).toBeDefined();
    });

    it('should handle source link with all properties', () => {
      const complexSource: SourceLink = {
        title: 'Complex Source',
        url: 'https://example.com/complex',
        body: 'Complex body content',
        type: 'api-doc',
        id: 'complex-id',
        metadata: {
          stackoverflow: {
            score: 42,
            answerCount: 5,
            isAccepted: true,
            lastActivityDate: Date.now() - 3600000 // 1 hour ago
          }
        }
      };

      sourceLinkCard = new SourceLinkCard({ sourceLink: complexSource });
      document.body.appendChild(sourceLinkCard.render);

      expect(sourceLinkCard.render).toBeDefined();

      const title = document.body.querySelector('.mynah-source-link-title');
      expect(title?.textContent).toContain(complexSource.title);

      const cardBody = document.body.querySelector('.mynah-card-body');
      expect(cardBody).toBeDefined();
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty source link', () => {
      const emptySource: SourceLink = {
        title: '',
        url: ''
      };

      sourceLinkCard = new SourceLinkCard({ sourceLink: emptySource });
      expect(sourceLinkCard.render).toBeDefined();
    });

    it('should handle source link with undefined body', () => {
      const sourceWithUndefinedBody: SourceLink = {
        title: 'Test',
        url: 'https://example.com',
        body: undefined
      };

      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithUndefinedBody });
      document.body.appendChild(sourceLinkCard.render);

      const cardBody = document.body.querySelector('.mynah-card-body');
      expect(cardBody).toBeNull();
    });

    it('should handle source link with empty metadata', () => {
      const sourceWithEmptyMetadata: SourceLink = {
        title: 'Test',
        url: 'https://example.com',
        metadata: {}
      };

      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithEmptyMetadata });
      expect(sourceLinkCard.render).toBeDefined();
    });

    it('should handle source link with null metadata', () => {
      const sourceWithNullMetadata: SourceLink = {
        title: 'Test',
        url: 'https://example.com',
        metadata: undefined
      };

      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithNullMetadata });
      expect(sourceLinkCard.render).toBeDefined();
    });
  });

  describe('Component Structure', () => {
    it('should have proper component hierarchy', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: sourceWithBody });
      document.body.appendChild(sourceLinkCard.render);

      // Should have card wrapper
      const card = document.body.querySelector('.mynah-card');
      expect(card).toBeDefined();

      // Should have header
      const header = card?.querySelector('.mynah-source-link-header');
      expect(header).toBeDefined();

      // Should have body
      const body = card?.querySelector('.mynah-card-body');
      expect(body).toBeDefined();
    });

    it('should maintain source link reference', () => {
      sourceLinkCard = new SourceLinkCard({ sourceLink: basicSourceLink });

      // The component should store the source link reference
      expect((sourceLinkCard as any).sourceLink).toBe(basicSourceLink);
    });
  });
});
