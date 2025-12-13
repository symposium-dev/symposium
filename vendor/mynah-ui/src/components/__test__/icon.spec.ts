import { Icon, MynahIcons } from '../icon';

describe('icon', () => {
  it('renders predefined icon', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
    });

    expect(testIcon.render).toBeDefined();
    expect(testIcon.render.classList.contains('mynah-ui-icon')).toBeTruthy();
    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop')).toBeTruthy();
  });

  it('renders globe icon', () => {
    const testIcon = new Icon({
      icon: MynahIcons.GLOBE,
    });

    expect(testIcon.render).toBeDefined();
    expect(testIcon.render.classList.contains('mynah-ui-icon-globe')).toBeTruthy();
  });

  it('renders icon with subtract', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
      subtract: true,
    });

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop-subtract')).toBeTruthy();
  });

  it('renders icon with status', () => {
    const testIcon = new Icon({
      icon: MynahIcons.GLOBE,
      status: 'success',
    });

    expect(testIcon.render.classList.contains('status-success')).toBeTruthy();
  });

  it('renders icon with custom classNames', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
      classNames: [ 'custom-class-1', 'custom-class-2' ],
    });

    expect(testIcon.render.classList.contains('custom-class-1')).toBeTruthy();
    expect(testIcon.render.classList.contains('custom-class-2')).toBeTruthy();
  });

  it('updates icon', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
    });

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop')).toBeTruthy();

    testIcon.update(MynahIcons.GLOBE);

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop')).toBeFalsy();
    expect(testIcon.render.classList.contains('mynah-ui-icon-globe')).toBeTruthy();
  });

  it('updates icon with subtract', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
      subtract: true,
    });

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop-subtract')).toBeTruthy();

    testIcon.update(MynahIcons.GLOBE);

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop-subtract')).toBeFalsy();
    expect(testIcon.render.classList.contains('mynah-ui-icon-globe-subtract')).toBeTruthy();
  });

  it('renders custom icon', () => {
    const customIcon = {
      name: 'custom-test-icon',
      base64Svg: 'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiPjwvc3ZnPg==',
    };

    const testIcon = new Icon({
      icon: customIcon,
    });

    expect(testIcon.render.classList.contains('mynah-ui-icon-custom-test-icon')).toBeTruthy();
  });

  it('updates from predefined to custom icon', () => {
    const testIcon = new Icon({
      icon: MynahIcons.DESKTOP,
    });

    const customIcon = {
      name: 'custom-icon',
      base64Svg: 'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMTYiIGhlaWdodD0iMTYiPjwvc3ZnPg==',
    };

    testIcon.update(customIcon);

    expect(testIcon.render.classList.contains('mynah-ui-icon-desktop')).toBeFalsy();
    expect(testIcon.render.classList.contains('mynah-ui-icon-custom-icon')).toBeTruthy();
  });
});
