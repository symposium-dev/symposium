import * as BaseConfigLight from './base-theme-light-config.json';
import * as BaseConfigDark from './base-theme-dark-config.json';
interface ConfigItem {
    type: 'measurement' | 'text' | 'color';
    description?: string;
    units?: string[];
    unit?: string;
    category?: string;
    alpha?: string;
    value: string;
}
const categories = [
    'sizing',
    'border-style',
    'font-size',
    'font-family',
    'text-color',
    'syntax-color',
    'status-color',
    'background-color',
    'shadow',
    'radius',
    'transition',
];

export class ThemeBuilder {
    private themeSelector: HTMLSelectElement = document.querySelector('#theme-selector') as HTMLSelectElement;
    private mainWrapper: HTMLElement = document.createElement('div');
    private inputsWrapper: HTMLElement = document.createElement('div');
    private buttonsWrapper: HTMLElement = document.createElement('div');
    private baseThemeType: 'light' | 'dark' = 'light';
    private currentConfig: Record<any, ConfigItem> = structuredClone(BaseConfigLight) as any;
    constructor(selector: string | HTMLElement) {
        delete this.currentConfig.default;
        this.themeSelector.addEventListener('change', e => {
            if (this.themeSelector.value.match('base-')) {
                this.baseThemeType = this.themeSelector.value.replace('base-', '') as 'light' | 'dark';
                if (this.baseThemeType === 'light') {
                    this.currentConfig = structuredClone(BaseConfigLight) as any;
                } else {
                    this.currentConfig = structuredClone(BaseConfigDark) as any;
                }
                this.inputsWrapper.innerHTML = '';
                this.fillInputWrapper();
                this.buildCssValues();
            } else if (this.themeSelector.value.match('dark-')) {
                document.querySelector('body')?.classList.add('vscode-dark');
            } else {
                document.querySelector('body')?.classList.remove('vscode-dark');
            }
            document.querySelector('html')?.setAttribute('theme', this.themeSelector.value);
        });
        this.mainWrapper.classList.add('mynah-ui-example-input-main-wrapper');
        this.inputsWrapper.classList.add('mynah-ui-example-input-items-wrapper');
        this.buttonsWrapper.classList.add('mynah-ui-example-input-buttons-wrapper');
        let parentWrapper: HTMLElement;
        if (typeof selector === 'string') {
            parentWrapper = document.querySelector(selector) ?? (document.querySelector('body') as HTMLElement);
        } else {
            parentWrapper = selector;
        }

        this.mainWrapper.insertAdjacentElement('beforeend', this.inputsWrapper);
        parentWrapper.insertAdjacentElement('beforeend', this.buttonsWrapper);
        parentWrapper.insertAdjacentElement('beforeend', this.mainWrapper);

        this.mainWrapper.insertAdjacentHTML(
            'beforeend',
            `
            <p>
                First, please select one of the <b>Custom Theme</b>s from the themes list on the header bar.
                After that you'll see the changes whenever you adjust one of the options below.</br>
                For measurement values (or anything other than colors) you can use current custom properties like the sizings.</br>
                First select (No Unit) option for the unit and then you can type any string into the value field. 
                And you can use custom properties as usual like <b>var(--mynah-sizing-1)</b> and the sizing values goes from <b>1 to 18</b>.
            </p>
            `
        );

        this.fillInputWrapper();

        const uploadThemeConfigFilePicker = document.createElement('input');
        uploadThemeConfigFilePicker.setAttribute('type', 'file');
        uploadThemeConfigFilePicker.setAttribute('accept', '.mynahuitc');
        uploadThemeConfigFilePicker.classList.add('hidden');
        uploadThemeConfigFilePicker.classList.add('config-operation');
        uploadThemeConfigFilePicker.classList.add('fill-state-always');
        uploadThemeConfigFilePicker.addEventListener('change', async () => {
            const file = uploadThemeConfigFilePicker.files?.item(0);
            if (file) {
                const text = await file.text();
                try {
                    this.currentConfig = JSON.parse(text);
                    this.inputsWrapper.innerHTML = '';
                    this.fillInputWrapper();
                    this.buildCssValues();
                    uploadThemeConfigFilePicker.value = '';
                } catch (err) {
                    console.warn("Coudln't read the JSON content");
                }
            }
        });

        const downloadThemeConfigButton = document.createElement('button');
        downloadThemeConfigButton.innerHTML = '<span>Download Config</span>';
        downloadThemeConfigButton.classList.add('mynah-button');
        downloadThemeConfigButton.classList.add('config-operation');
        downloadThemeConfigButton.classList.add('fill-state-always');
        downloadThemeConfigButton.addEventListener('click', () => {
            download('mynah-ui-theme.mynahuitc', JSON.stringify(this.currentConfig));
        });

        const resetThemeConfigButton = document.createElement('button');
        resetThemeConfigButton.innerHTML = '<span>Reset</span>';
        resetThemeConfigButton.classList.add('mynah-button');
        resetThemeConfigButton.classList.add('config-operation');
        resetThemeConfigButton.classList.add('fill-state-always');
        resetThemeConfigButton.addEventListener('click', () => {
            this.currentConfig = structuredClone(this.baseThemeType === 'light' ? BaseConfigLight : BaseConfigDark) as any;
            this.inputsWrapper.innerHTML = '';
            this.fillInputWrapper();
            this.buildCssValues();
        });

        const uploadThemeConfigButton = document.createElement('button');
        uploadThemeConfigButton.innerHTML = '<span>Upload Config</span>';
        uploadThemeConfigButton.classList.add('mynah-button');
        uploadThemeConfigButton.classList.add('config-operation');
        uploadThemeConfigButton.classList.add('fill-state-always');
        uploadThemeConfigButton.addEventListener('click', () => {
            uploadThemeConfigFilePicker.click();
        });

        const downloadThemeButton = document.createElement('button');
        downloadThemeButton.innerHTML = '<span>Download Theme (CSS)</span>';
        downloadThemeButton.classList.add('mynah-button');
        downloadThemeButton.classList.add('config-operation');
        downloadThemeButton.classList.add('fill-state-always');
        downloadThemeButton.addEventListener('click', () => {
            download(
                'mynah-ui-theme.css',
                `:root {
                ${this.getCssCustomVars()}
            }`
            );
        });
        this.buttonsWrapper.insertAdjacentElement('beforeend', uploadThemeConfigFilePicker);
        this.buttonsWrapper.insertAdjacentElement('beforeend', uploadThemeConfigButton);
        this.buttonsWrapper.insertAdjacentElement('beforeend', downloadThemeConfigButton);
        this.buttonsWrapper.insertAdjacentElement('beforeend', downloadThemeButton);
        this.buttonsWrapper.insertAdjacentElement('beforeend', resetThemeConfigButton);

        this.buildCssValues();
    }

    private fillInputWrapper = () => {
        categories.forEach(category => {
            this.inputsWrapper.insertAdjacentHTML(
                'beforeend',
                `
                <div class="mynah-ui-example-input mynah-ui-example-input-category-${category}">
                <h1>${category}</h1>
                </div>
            `
            );
        });

        Object.keys(this.currentConfig).forEach((themeConfigKey: string) => {
            const themeConfigItem = this.currentConfig[themeConfigKey] as ConfigItem;
            switch (themeConfigItem.type) {
                case 'text':
                    this.inputsWrapper.insertAdjacentElement(
                        'beforeend',
                        themeInputText(themeConfigKey, themeConfigItem, value => {
                            this.currentConfig[themeConfigKey].value = value;
                            this.buildCssValues();
                        })
                    );
                    break;
                case 'measurement':
                    this.inputsWrapper.insertAdjacentElement(
                        'beforeend',
                        themeInputMeasurement(themeConfigKey, themeConfigItem, (value, unit) => {
                            this.currentConfig[themeConfigKey].value = value;
                            this.currentConfig[themeConfigKey].unit = unit;
                            this.buildCssValues();
                        })
                    );
                    break;
                case 'color':
                    this.inputsWrapper.insertAdjacentElement(
                        'beforeend',
                        themeInputColor(themeConfigKey, themeConfigItem, (hex, alpha) => {
                            this.currentConfig[themeConfigKey].value = hex;
                            this.currentConfig[themeConfigKey].alpha = alpha;
                            this.buildCssValues();
                        })
                    );
                    break;
            }
        });
    };

    private buildCssValues = () => {
        (document.querySelector('#custom-style') as HTMLElement).innerHTML = `
        html[theme="base-${this.baseThemeType}"]:root {
            font-size: 13px;
            ${this.getCssCustomVars()}
        }
        `;
    };

    private getCssCustomVars = (): string =>
        Object.keys(this.currentConfig)
            .map(configKey => {
                const configItem = this.currentConfig[configKey];
                let value = configItem.value;
                switch (configItem.type) {
                    case 'measurement':
                        value = value + configItem.unit;
                        break;
                    case 'color':
                        value = getColorValue(value, configItem.alpha ?? '100');
                        break;
                }
                return `${configKey}: ${value};`;
            })
            .join('\n');
}

const getCleanTitle = (title: string): string => {
    return title.replace('--mynah-', '').split('-').join(' ');
};

const getColorValue = (hex: string, alpha: string): string => {
    const realAlpha = parseInt(alpha);
    if (realAlpha === 100) {
        return hex;
    } else {
        let hexToUse = hex.length === 4 ? hex[0] + hex.slice(1, 4).repeat(2) : hex;
        var r = parseInt(hexToUse.slice(1, 3), 16),
            g = parseInt(hexToUse.slice(3, 5), 16),
            b = parseInt(hexToUse.slice(5, 7), 16);

        return 'rgba(' + r + ', ' + g + ', ' + b + ', ' + (parseInt(alpha) / 100).toString() + ')';
    }
};

const themeInputText = (title: string, configItem: ConfigItem, onValueChange: (value: string) => void): HTMLDivElement => {
    const element = document.createElement('div');
    element.classList.add('mynah-ui-example-input');
    element.classList.add(`mynah-ui-example-input-category-${configItem.category ?? 'other'}`);

    element.innerHTML = `
        <div class="mynah-ui-example-input-title-wrapper"><h4>${getCleanTitle(title)}</h4>
        <span>${configItem.description ?? ''}</span></div>
    `;

    const inputElement = document.createElement('input');
    inputElement.setAttribute('type', 'text');
    inputElement.setAttribute('value', configItem.value);
    inputElement.addEventListener('change', e => {
        onValueChange(inputElement.value);
    });

    const inputElementWrapper = document.createElement('div');
    inputElementWrapper.classList.add('mynah-ui-example-input-wrapper');
    inputElementWrapper.insertAdjacentElement('beforeend', inputElement);

    element.insertAdjacentElement('beforeend', inputElementWrapper);

    return element as HTMLDivElement;
};

const themeInputMeasurement = (title: string, configItem: ConfigItem, onValueChange: (value: string, unit: string) => void): HTMLDivElement => {
    const element = document.createElement('div');
    element.classList.add('mynah-ui-example-input');
    element.classList.add(`mynah-ui-example-input-category-${configItem.category ?? 'other'}`);

    element.innerHTML = `
        <div class="mynah-ui-example-input-title-wrapper"><h4>${getCleanTitle(title)}</h4>
        <span>${configItem.description ?? ''}</span></div>
    `;

    const selectElement = document.createElement('select');
    configItem.units?.forEach(unitKey => {
        selectElement.insertAdjacentHTML(
            'beforeend',
            `
            <option ${configItem.unit === unitKey ? 'selected' : ''} value="${unitKey}">${unitKey !== '' ? unitKey : '?'}</option>
            `
        );
    });
    selectElement.addEventListener('change', e => {
        inputElement.setAttribute('type', selectElement.value === '' ? 'text' : 'number');
        onValueChange(inputElement.value, selectElement.value);
    });

    const inputElement = document.createElement('input');
    inputElement.setAttribute('type', configItem.unit === '' ? 'text' : 'number');
    inputElement.setAttribute('value', configItem.value);
    inputElement.addEventListener('change', e => {
        onValueChange(inputElement.value, selectElement.value);
    });

    const inputElementWrapper = document.createElement('div');
    inputElementWrapper.classList.add('mynah-ui-example-input-wrapper');
    inputElementWrapper.insertAdjacentElement('beforeend', inputElement);
    inputElementWrapper.insertAdjacentElement('beforeend', selectElement);

    element.insertAdjacentElement('beforeend', inputElementWrapper);

    return element as HTMLDivElement;
};

const themeInputColor = (title: string, configItem: ConfigItem, onValueChange: (hex: string, alpha: string) => void): HTMLDivElement => {
    const element = document.createElement('div');
    element.classList.add('mynah-ui-example-input');
    element.classList.add(`mynah-ui-example-input-category-${configItem.category ?? 'other'}`);
    const splittedValue = {
        hex: configItem.value,
        alpha: configItem.alpha ?? '100',
    };

    element.innerHTML = `
        <div class="mynah-ui-example-input-title-wrapper"><h4>${getCleanTitle(title)}</h4>
        <span>${configItem.description ?? ''}</span></div>
    `;

    const alphaSlider = document.createElement('input');
    alphaSlider.setAttribute('type', 'range');
    alphaSlider.setAttribute('min', '0');
    alphaSlider.setAttribute('max', '100');
    alphaSlider.setAttribute('value', splittedValue.alpha ?? '100');
    alphaSlider.addEventListener('change', e => {
        onValueChange(inputElement.value, alphaSlider.value);
        (inputElementLabelWrapper.querySelector('small[type="range"] > b') as HTMLElement).innerHTML = `${alphaSlider.value}%`;
    });

    const inputElement = document.createElement('input');
    inputElement.setAttribute('type', 'color');
    inputElement.setAttribute('value', splittedValue.hex);
    inputElement.addEventListener('change', e => {
        onValueChange(inputElement.value, alphaSlider.value);
        (inputElementLabelWrapper.querySelector('small[type="color"] > b') as HTMLElement).innerHTML = inputElement.value;
    });

    const inputElementLabelWrapper = document.createElement('div');
    inputElementLabelWrapper.classList.add('mynah-ui-example-input-wrapper');
    inputElementLabelWrapper.insertAdjacentHTML(
        'beforeend',
        `
    <small type="color">Color: <b>${configItem.value}</b></small>
    <small type="range">Alpha: <b>${configItem.alpha ?? 100}%</b></small>
    `
    );

    const inputElementWrapper = document.createElement('div');
    inputElementWrapper.classList.add('mynah-ui-example-input-wrapper');
    inputElementWrapper.insertAdjacentElement('beforeend', inputElement);
    inputElementWrapper.insertAdjacentElement('beforeend', alphaSlider);

    element.insertAdjacentElement('beforeend', inputElementLabelWrapper);
    element.insertAdjacentElement('beforeend', inputElementWrapper);

    return element as HTMLDivElement;
};

const download = (filename: string, text: string) => {
    var element = document.createElement('a');
    element.setAttribute('href', 'data:text/plain;charset=utf-8,' + encodeURIComponent(text));
    element.setAttribute('download', filename);

    element.style.display = 'none';
    document.body.appendChild(element);

    element.click();

    document.body.removeChild(element);
};
