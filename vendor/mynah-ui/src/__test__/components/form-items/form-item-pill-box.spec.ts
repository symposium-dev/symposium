import { FormItemPillBox } from '../../../components/form-items/form-item-pill-box';

describe('FormItemPillBox', () => {
  let pillBox: FormItemPillBox;

  beforeEach(() => {
    pillBox = new FormItemPillBox({
      id: 'test-pill-box',
      label: 'Test Pills',
      placeholder: 'Add a pill'
    });
    document.body.appendChild(pillBox.render);
  });

  afterEach(() => {
    document.body.innerHTML = '';
  });

  it('should render pill box', () => {
    expect(pillBox.render).toBeDefined();
    expect(pillBox.render.querySelector('.mynah-form-item-pill-box-wrapper')).toBeTruthy();
  });

  it('should add pill on enter', () => {
    const input = pillBox.render.querySelector('.mynah-form-item-pill-box-input') as HTMLTextAreaElement;
    input.value = 'test-pill';

    const event = new KeyboardEvent('keydown', { key: 'Enter' });
    input.dispatchEvent(event);

    expect(pillBox.getValue()).toBe('test-pill');
    expect(pillBox.render.querySelector('.mynah-form-item-pill')).toBeTruthy();
  });

  it('should remove pill on click', () => {
    pillBox.setValue('pill1,pill2');

    const removeButton = pillBox.render.querySelector('.mynah-form-item-pill-remove') as HTMLElement;
    removeButton.click();

    expect(pillBox.getValue()).toBe('pill2');
  });

  it('should set and get values', () => {
    pillBox.setValue('tag1,tag2,tag3');
    expect(pillBox.getValue()).toBe('tag1,tag2,tag3');
  });

  it('should disable component', () => {
    pillBox.setEnabled(false);
    expect(pillBox.render.hasAttribute('disabled')).toBe(true);
  });
});
