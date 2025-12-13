import { MynahPortalNames } from '../../static';
import { DomBuilder, DomBuilderObject, DomBuilderObjectFilled } from '../dom';

describe('dom', () => {
  describe('DomBuilder', () => {
    it('build a basic element', () => {
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        attributes: { id: '#testDiv1', draggable: 'true' },
        classNames: [ 'test-class-1', 'test-class-2' ],
        innerHTML: 'innerHTML string',
      };
      const resultElement = DomBuilder.getInstance().build(mockDomBuilderObject);
      expect(resultElement.id).toBe('#testDiv1');
      expect(resultElement.draggable).toBe(true);
      expect(Object.values(resultElement.classList)).toEqual([ 'test-class-1', 'test-class-2' ]);
      expect(resultElement.innerHTML).toBe('innerHTML string');
    });

    it('build an element with children', () => {
      const domBuilder = DomBuilder.getInstance();
      const mockChildElementBuilderObject: DomBuilderObject = {
        type: 'span',
        attributes: { id: '#childSpan1' },
      };
      const childElement = domBuilder.build(mockChildElementBuilderObject);

      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        attributes: { id: '#testDiv1', draggable: 'true' },
        classNames: [ 'test-class-1', 'test-class-2' ],
        children: [ childElement ],
      };
      const resultElement = domBuilder.build(mockDomBuilderObject);
      expect(resultElement.childNodes).toHaveLength(1);
      expect(resultElement.children[0].outerHTML).toBe('<span id="#childSpan1"></span>');
    });

    it('update an element', () => {
      const domBuilder = DomBuilder.getInstance();
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        attributes: { id: '#testDiv1', draggable: 'true' },
        classNames: [ 'test-class-1', 'test-class-2' ],
        innerHTML: 'innerHTML string',
      };
      const initialElement = domBuilder.build(mockDomBuilderObject);
      expect(initialElement.id).toBe('#testDiv1');
      expect(initialElement.draggable).toBe(true);
      expect(Object.values(initialElement.classList)).toEqual([ 'test-class-1', 'test-class-2' ]);
      expect(initialElement.innerHTML).toBe('innerHTML string');

      const mockUpdatedDomBuilderObject: DomBuilderObjectFilled = {
        attributes: { id: '#testDiv2' },
        classNames: [ 'test-class-3' ],
        innerHTML: '',
      };
      const updatedElement = domBuilder.update(initialElement, mockUpdatedDomBuilderObject);
      expect(updatedElement.id).toBe('#testDiv2');
      expect(updatedElement.draggable).toBe(true);
      expect(Object.values(updatedElement.classList)).toEqual([ 'test-class-3' ]);
      expect(updatedElement.innerHTML).toBe('');
    });
  });

  describe('ExtendedHTMLElement', () => {
    it('addClass', () => {
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        classNames: [ 'test-class-1' ],
      };
      const builtElement = DomBuilder.getInstance().build(mockDomBuilderObject);
      builtElement.addClass('test-class-2');
      expect(Object.values(builtElement.classList)).toEqual([ 'test-class-1', 'test-class-2' ]);
    });

    it('removeClass', () => {
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        classNames: [ 'test-class-1', 'test-class-2' ],
      };
      const builtElement = DomBuilder.getInstance().build(mockDomBuilderObject);
      builtElement.removeClass('test-class-1');
      expect(Object.values(builtElement.classList)).toEqual([ 'test-class-2' ]);
    });

    it('toggleClass', () => {
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
      };
      const builtElement = DomBuilder.getInstance().build(mockDomBuilderObject);
      builtElement.toggleClass('test-class-1');
      expect(Object.values(builtElement.classList)).toEqual([ 'test-class-1' ]);
      builtElement.toggleClass('test-class-1');
      expect(Object.values(builtElement.classList)).toEqual([]);
    });

    it('hasClass', () => {
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        classNames: [ 'test-class-1', 'test-class-2' ],
      };
      const resultElement = DomBuilder.getInstance().build(mockDomBuilderObject);
      expect(Object.values(resultElement.classList).includes('test-class-1')).toBeTruthy();
      expect(Object.values(resultElement.classList).includes('test-class-2')).toBeTruthy();
      expect(Object.values(resultElement.classList).includes('test-class-3')).toBeFalsy();
    });

    it('insertChild', () => {
      const domBuilder = DomBuilder.getInstance();
      const mockDomBuilderObject: DomBuilderObject = {
        type: 'div',
        attributes: { id: '#testDiv1' },
        classNames: [ 'test-class-1', 'test-class-2' ],
      };
      const resultElement = domBuilder.build(mockDomBuilderObject);

      const childElement1 = domBuilder.build({
        type: 'span',
        attributes: { id: '#childSpan1' },
      });
      resultElement.insertChild('beforeend', childElement1);

      const childElement2 = domBuilder.build({
        type: 'span',
        attributes: { id: '#childSpan2' },
      });
      resultElement.insertChild('afterbegin', childElement2);

      expect(resultElement.childNodes).toHaveLength(2);
      expect(resultElement.children[0].outerHTML).toBe('<span id="#childSpan2"></span>');
      expect(resultElement.children[1].outerHTML).toBe('<span id="#childSpan1"></span>');
    });

    it('remove (children)', () => {
      const domBuilder = DomBuilder.getInstance();
      const childElement1 = domBuilder.build({
        type: 'span',
        attributes: { id: '#childSpan1' },
      });

      const childElement2 = domBuilder.build({
        type: 'span',
        attributes: { id: '#childSpan2' },
        // should prevent it from being removed
        persistent: true,
      });

      const resultElement = domBuilder.build({
        type: 'div',
        attributes: { id: '#testDiv1' },
        children: [ childElement1, childElement2 ],
      });

      expect(resultElement.childNodes).toHaveLength(2);
      expect(resultElement.children[0].outerHTML).toBe('<span id="#childSpan1"></span>');
      expect(resultElement.children[1].outerHTML).toBe('<span id="#childSpan2"></span>');

      resultElement.clear();
      expect(resultElement.childNodes).toHaveLength(1);
      expect(resultElement.children[0].outerHTML).toBe('<span id="#childSpan2"></span>');
    });
  });

  describe('portal', () => {
    it('createPortal', () => {
      document.body.innerHTML = '<div id="#existingDiv" />';

      const domBuilder = DomBuilder.getInstance();
      domBuilder.createPortal(
        'testPortal',
        {
          type: 'div',
          attributes: {
            id: '#wrapper1',
          },
          classNames: [ 'wrapper-class-1' ]
        },
        'afterbegin'
      );

      expect(document.body.children).toHaveLength(2);
      expect(document.body.children[0].outerHTML).toBe('<div class="wrapper-class-1" id="#wrapper1"></div>');
      expect(document.body.children[1].outerHTML).toBe('<div id="#existingDiv"></div>');
    });

    it('getPortal', () => {
      document.body.innerHTML = '<div id="#existingDiv" />';

      const domBuilder = DomBuilder.getInstance();
      domBuilder.createPortal(
        'testPortal',
        {
          type: 'div',
          attributes: {
            id: '#wrapper1',
          },
          classNames: [ 'wrapper-class-1' ]
        },
        'afterbegin'
      );

      expect(domBuilder.getPortal('testPortal')?.id).toBe('#wrapper1');
    });

    it('removePortal', () => {
      document.body.innerHTML = '<div id="#existingDiv" />';

      const domBuilder = DomBuilder.getInstance();
      domBuilder.createPortal(
        'testPortal',
        {
          type: 'div',
          attributes: {
            id: '#wrapper1',
          },
          classNames: [ 'wrapper-class-1' ]
        },
        'afterbegin'
      );

      domBuilder.removePortal('testPortal');

      expect(document.body.children).toHaveLength(1);
      expect(document.body.children[0].outerHTML).toBe('<div id="#existingDiv"></div>');
    });

    it('removeAllPortals', () => {
      document.body.innerHTML = '<div id="#existingDiv" />';

      const domBuilder = DomBuilder.getInstance();
      domBuilder.createPortal(
        'wrapper1',
        {
          type: 'div',
          attributes: {
            id: '#wrapper1',
          },
          classNames: [ 'wrapper-class-1' ]
        },
        'afterbegin'
      );
      domBuilder.createPortal(
        'wrapper2',
        {
          type: 'div',
          attributes: {
            id: '#wrapper2',
          },
          classNames: [ 'wrapper-class-2' ]
        },
        'afterbegin'
      );

      expect(document.body.children).toHaveLength(3);
      expect(document.body.children[0].outerHTML).toBe('<div class="wrapper-class-2" id="#wrapper2"></div>');
      expect(document.body.children[1].outerHTML).toBe('<div class="wrapper-class-1" id="#wrapper1"></div>');
      expect(document.body.children[2].outerHTML).toBe('<div id="#existingDiv"></div>');

      domBuilder.removeAllPortals(MynahPortalNames.WRAPPER);

      expect(document.body.children).toHaveLength(1);
      expect(document.body.children[0].outerHTML).toBe('<div id="#existingDiv"></div>');
    });
  });
});
