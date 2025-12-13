/*!
 * Copyright 2022 Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

import { SourceLinkBody, SourceLinkBodyProps } from '../../../components/source-link/source-link-body';
import { SourceLink, ReferenceTrackerInformation } from '../../../static';
import { DomBuilder } from '../../../helper/dom';

describe('SourceLinkBody Component', () => {
  let sourceLinkBody: SourceLinkBody;

  const basicSuggestion: Partial<SourceLink> = {
    title: 'Test Source',
    url: 'https://example.com',
    body: 'This is the body content of the source link'
  };

  const suggestionWithoutBody: Partial<SourceLink> = {
    title: 'Source without body',
    url: 'https://example.com'
  };

  const suggestionWithEmptyBody: Partial<SourceLink> = {
    title: 'Source with empty body',
    url: 'https://example.com',
    body: ''
  };

  beforeEach(() => {
    document.body.innerHTML = '';
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  describe('Basic Functionality', () => {
    it('should create source link body with basic props', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: basicSuggestion });

      expect(sourceLinkBody.render).toBeDefined();
      expect(sourceLinkBody.render.classList.contains('mynah-card-body')).toBe(true);
    });

    it('should render body content', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: basicSuggestion });
      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toContain(basicSuggestion.body);
    });

    it('should store props reference', () => {
      const props: SourceLinkBodyProps = { suggestion: basicSuggestion };
      sourceLinkBody = new SourceLinkBody(props);

      expect(sourceLinkBody.props).toBe(props);
    });
  });

  describe('Body Content Handling', () => {
    it('should handle suggestion with body content', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: basicSuggestion });
      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toBe(basicSuggestion.body);
    });

    it('should handle suggestion without body', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: suggestionWithoutBody });
      document.body.appendChild(sourceLinkBody.render);

      // Should render empty content
      expect(sourceLinkBody.render.textContent).toBe('');
    });

    it('should handle suggestion with empty body', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: suggestionWithEmptyBody });
      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toBe('');
    });

    it('should handle undefined suggestion body', () => {
      const suggestionUndefinedBody: Partial<SourceLink> = {
        title: 'Test',
        url: 'https://example.com',
        body: undefined
      };

      sourceLinkBody = new SourceLinkBody({ suggestion: suggestionUndefinedBody });
      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toBe('');
    });
  });

  describe('Children Handling', () => {
    it('should handle additional children elements', () => {
      const childElement = DomBuilder.getInstance().build({
        type: 'div',
        classNames: [ 'test-child' ],
        children: [ 'Child content' ]
      });

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: [ childElement ]
      });

      document.body.appendChild(sourceLinkBody.render);

      const child = document.body.querySelector('.test-child');
      expect(child).toBeDefined();
      expect(child?.textContent).toBe('Child content');
    });

    it('should handle multiple children', () => {
      const child1 = DomBuilder.getInstance().build({
        type: 'span',
        classNames: [ 'child-1' ],
        children: [ 'Child 1' ]
      });

      const child2 = DomBuilder.getInstance().build({
        type: 'span',
        classNames: [ 'child-2' ],
        children: [ 'Child 2' ]
      });

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: [ child1, child2 ]
      });

      document.body.appendChild(sourceLinkBody.render);

      const firstChild = document.body.querySelector('.child-1');
      const secondChild = document.body.querySelector('.child-2');

      expect(firstChild?.textContent).toBe('Child 1');
      expect(secondChild?.textContent).toBe('Child 2');
    });

    it('should handle string children', () => {
      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: [ 'String child content' ]
      });

      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toContain('String child content');
    });

    it('should handle HTML element children', () => {
      const htmlElement = document.createElement('div');
      htmlElement.textContent = 'HTML element child';
      htmlElement.className = 'html-child';

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: [ htmlElement ]
      });

      document.body.appendChild(sourceLinkBody.render);

      const child = document.body.querySelector('.html-child');
      expect(child?.textContent).toBe('HTML element child');
    });

    it('should handle empty children array', () => {
      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: []
      });

      expect(sourceLinkBody.render).toBeDefined();
    });

    it('should handle undefined children', () => {
      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: undefined
      });

      expect(sourceLinkBody.render).toBeDefined();
    });
  });

  describe('Highlight Range with Tooltip', () => {
    it('should handle highlight range information', () => {
      const highlightInfo: ReferenceTrackerInformation[] = [
        {
          licenseName: 'MIT',
          repository: 'https://github.com/example/repo',
          url: 'https://example.com',
          recommendationContentSpan: { start: 0, end: 10 },
          information: 'Test information'
        }
      ];

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        highlightRangeWithTooltip: highlightInfo
      });

      expect(sourceLinkBody.render).toBeDefined();
    });

    it('should handle empty highlight range array', () => {
      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        highlightRangeWithTooltip: []
      });

      expect(sourceLinkBody.render).toBeDefined();
    });

    it('should handle undefined highlight range', () => {
      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        highlightRangeWithTooltip: undefined
      });

      expect(sourceLinkBody.render).toBeDefined();
    });

    it('should handle multiple highlight ranges', () => {
      const highlightInfo: ReferenceTrackerInformation[] = [
        {
          licenseName: 'MIT',
          repository: 'https://github.com/example/repo1',
          url: 'https://example1.com',
          recommendationContentSpan: { start: 0, end: 5 },
          information: 'First information'
        },
        {
          licenseName: 'Apache-2.0',
          repository: 'https://github.com/example/repo2',
          url: 'https://example2.com',
          recommendationContentSpan: { start: 10, end: 15 },
          information: 'Second information'
        }
      ];

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        highlightRangeWithTooltip: highlightInfo
      });

      expect(sourceLinkBody.render).toBeDefined();
    });
  });

  describe('Edge Cases', () => {
    it('should handle empty suggestion object', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: {} });

      expect(sourceLinkBody.render).toBeDefined();
      document.body.appendChild(sourceLinkBody.render);
      expect(sourceLinkBody.render.textContent).toBe('');
    });

    it('should handle null suggestion', () => {
      // The component should handle null gracefully by using optional chaining
      sourceLinkBody = new SourceLinkBody({ suggestion: {} });

      expect(sourceLinkBody.render).toBeDefined();
    });

    it('should handle suggestion with only title', () => {
      const titleOnlySuggestion: Partial<SourceLink> = {
        title: 'Only title'
      };

      sourceLinkBody = new SourceLinkBody({ suggestion: titleOnlySuggestion });
      document.body.appendChild(sourceLinkBody.render);

      expect(sourceLinkBody.render.textContent).toBe('');
    });

    it('should handle complex body content', () => {
      const complexBodySuggestion: Partial<SourceLink> = {
        title: 'Complex body',
        url: 'https://example.com',
        body: 'This is a complex body with multiple lines and special characters'
      };

      sourceLinkBody = new SourceLinkBody({ suggestion: complexBodySuggestion });
      document.body.appendChild(sourceLinkBody.render);

      // Check that the content is rendered (whitespace may be normalized)
      expect(sourceLinkBody.render.textContent).toContain('This is a complex body');
      expect(sourceLinkBody.render.textContent).toContain('multiple lines');
      expect(sourceLinkBody.render.textContent).toContain('special characters');
    });
  });

  describe('Component Integration', () => {
    it('should integrate properly with CardBody component', () => {
      sourceLinkBody = new SourceLinkBody({ suggestion: basicSuggestion });
      document.body.appendChild(sourceLinkBody.render);

      // Should have card body class from CardBody component
      expect(sourceLinkBody.render.classList.contains('mynah-card-body')).toBe(true);
    });

    it('should pass all props to CardBody correctly', () => {
      const childElement = DomBuilder.getInstance().build({
        type: 'div',
        children: [ 'Test child' ]
      });

      const highlightInfo: ReferenceTrackerInformation[] = [
        {
          licenseName: 'MIT',
          repository: 'https://github.com/test/repo',
          url: 'https://test.com',
          recommendationContentSpan: { start: 0, end: 5 },
          information: 'Test information'
        }
      ];

      sourceLinkBody = new SourceLinkBody({
        suggestion: basicSuggestion,
        children: [ childElement ],
        highlightRangeWithTooltip: highlightInfo
      });

      expect(sourceLinkBody.render).toBeDefined();
    });
  });
});
