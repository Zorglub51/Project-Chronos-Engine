/* Per-game controls. No engine defaults are written merely by opening a game. */
(() => {
    'use strict';
    const own = (object, key) => Object.prototype.hasOwnProperty.call(object, key);
    const get = (game, path) => path.split('.').reduce((value, key) => value?.[key], game);

    function validate(field, value) {
        let typed = value;
        if (field.allowNumericString && typeof value === 'string' && /^-?\d+$/.test(value)) typed = Number(value);
        if (field.type === 'integer' && (!Number.isInteger(typed) || typed < -2147483648 || typed > 2147483647)) return 'Enter a 32-bit whole number.';
        if (field.type === 'number' && (typeof typed !== 'number' || !Number.isFinite(typed) || Math.abs(typed) > 3.4028234663852886e38)) return 'Enter a finite number supported by the engine.';
        if (field.type === 'boolean' && typeof typed !== 'boolean') return 'Choose true or false.';
        if (field.type === 'string' && (typeof typed !== 'string' || typed.includes('\0'))) return 'Enter text without null characters.';
        if (field.type === 'array' && !Array.isArray(typed)) return 'Enter a JSON array.';
        if (field.type === 'object' && (!typed || Array.isArray(typed) || typeof typed !== 'object')) return 'Enter a JSON object.';
        if (field.type === 'object' && Object.keys(typed).some(key => key === '__chronos_array' || key.includes('\0'))) return 'Attribute names cannot contain null characters or use the reserved key __chronos_array.';
        if (field.min != null && typed < field.min) return `Minimum: ${field.min}.`;
        if (field.max != null && typed > field.max) return `Maximum: ${field.max}.`;
        if (field.shape === 'split' && (typed.length !== 2 || !typed.every(n => Number.isInteger(n) && n >= 1 && n <= 16))) return 'Use two whole numbers from 1 to 16, for example [1, 1].';
        if (field.shape === 'positiveNumbers' && (!typed.length || typed.length > 32 || !typed.every(n => Number.isInteger(n) && n > 0 && n <= 60))) return 'Use 1–32 whole-number frequencies from 1 to 60.';
        if (field.shape === 'integerMap' && !Object.values(typed).every(n => Number.isInteger(n) && n >= -2147483648 && n <= 2147483647)) return 'Every attribute must be a 32-bit whole number.';
        if (field.shape === 'stringMap' && !Object.values(typed).every(n => typeof n === 'string' && !n.includes('\0'))) return 'Every value must be text without null characters.';
        if (field.shape === 'tag' && typed !== '' && typed !== 'NO' && !/^[A-Za-z0-9_]{1,14}$/.test(typed)) return 'Use NO, or an ASCII tag of at most 14 letters, digits or underscores.';
        return '';
    }

    function parse(field, text) {
        if (text === '' && field.optional) return undefined;
        if (text === '' && (field.type === 'integer' || field.type === 'number')) throw new Error('Enter a number.');
        let value = text;
        if (field.type === 'integer' || field.type === 'number') value = Number(text);
        if (field.type === 'boolean') value = text === 'true' ? true : text === 'false' ? false : null;
        if (field.type === 'array' || field.type === 'object') value = JSON.parse(text);
        const error = validate(field, value);
        if (error) throw new Error(error);
        return value;
    }

    function assign(game, field, value) {
        const [block, key] = field.path.split('.');
        if (value === undefined) {
            if (game[block]) delete game[block][key];
        } else {
            if (!game[block]) game[block] = {};
            game[block][key] = value;
        }
    }

    function possibleValues(field) {
        if (field.values) return field.values;
        if (field.choices) return field.choices.map(([value, label]) => `${value}: ${label}`).join('; ');
        if (field.type === 'boolean') return 'true or false';
        if (field.shape === 'split') return '[X, Y], two positive whole numbers. Editor range: 1–16 per axis.';
        if (field.shape === 'positiveNumbers') return 'JSON array of frequencies. Editor range: 1–32 entries, each a whole number from 1 to 60.';
        if (field.shape === 'integerMap') return 'JSON object with text keys and signed 32-bit integer values.';
        if (field.shape === 'stringMap') return 'JSON object with text keys and text values.';
        if (field.shape === 'tag') return 'NO or blank to disable; otherwise 1–14 ASCII letters, digits or underscores.';
        if (field.type === 'integer') return `Whole number, ${field.min ?? -2147483648} to ${field.max ?? 2147483647}.`;
        if (field.type === 'number') return `Finite number${field.min != null ? `, at least ${field.min}` : ''}; decimals are accepted.`;
        return field.type === 'string' ? 'Text.' : `JSON ${field.type}.`;
    }

    function infoBubble(field) {
        const wrapper = document.createElement('span');
        wrapper.className = 'parameter-info';
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'parameter-info-button';
        button.textContent = 'i';
        button.setAttribute('aria-label', 'Information about ' + field.label);
        button.setAttribute('aria-expanded', 'false');
        const tooltip = document.createElement('span');
        tooltip.className = 'parameter-tooltip';
        tooltip.id = 'parameter-info-' + (field.path || field.key).replaceAll('.', '-');
        tooltip.setAttribute('role', 'tooltip');
        tooltip.hidden = true;
        button.setAttribute('aria-describedby', tooltip.id);
        button.setAttribute('aria-controls', tooltip.id);
        const update = field => {
            tooltip.replaceChildren();
            for (const text of [field.help, 'Possible values: ' + possibleValues(field),
                ...(own(field, 'default') ? ['Default: ' + JSON.stringify(field.default) + '.'] : []),
                ...(field.ui === 'expert' && field.optional ? [field.path.startsWith('rom.')
                    ? 'Leave blank or choose Inherit to use the shared profile setting.'
                    : 'Leave blank or reset this field to use its standard menu value.'] : [])]) {
                const paragraph = document.createElement('span');
                paragraph.className = 'parameter-tooltip-paragraph';
                paragraph.textContent = text;
                tooltip.append(paragraph);
            }
        };
        wrapper.update = update;
        update(field);
        let pinned = false;
        const show = () => { tooltip.hidden = false; button.setAttribute('aria-expanded', 'true'); };
        const hide = () => { tooltip.hidden = true; button.setAttribute('aria-expanded', 'false'); };
        wrapper.addEventListener('pointerenter', show);
        wrapper.addEventListener('pointerleave', () => { if (!pinned && document.activeElement !== button) hide(); });
        button.addEventListener('focus', show);
        button.addEventListener('blur', () => { if (!pinned) hide(); });
        button.addEventListener('click', event => {
            event.preventDefault();
            event.stopPropagation();
            pinned = !pinned;
            if (pinned) show(); else hide();
        });
        button.addEventListener('keydown', event => {
            if (event.key === 'Escape') { pinned = false; hide(); event.stopPropagation(); }
        });
        document.addEventListener?.('pointerdown', event => {
            if (!wrapper.contains(event.target)) { pinned = false; hide(); }
        });
        wrapper.append(button, tooltip);
        return wrapper;
    }

    function decorateDetails(catalog) {
        const aliases = { 'display.titlebar': 'f-titlebar', 'display.ccolor': 'f-ccolor' };
        for (const field of catalog.parameters.filter(field => field.ui === 'existing')) {
            const control = document.querySelector(`[data-path="${field.path}"]`)
                || (aliases[field.path] ? document.getElementById(aliases[field.path]) : null);
            const label = control?.closest('.field-row')?.querySelector('.field-label');
            if (label && !label.dataset.parameterInfo) {
                label.append(infoBubble(field));
                label.dataset.parameterInfo = 'true';
            }
        }
    }

    function create(container, catalog, onChange) {
        let current = null;
        const groups = [...new Set(catalog.parameters.filter(field => field.ui === 'expert').map(field => field.group))];
        const fields = groups.flatMap(group => catalog.parameters.filter(field => field.ui === 'expert' && field.group === group));
        const controls = [];
        const make = (tag, text, className) => {
            const element = document.createElement(tag);
            if (text != null) element.textContent = text;
            if (className) element.className = className;
            return element;
        };
        container.replaceChildren();
        let group = null;
        for (const field of fields) {
            const inheritsProfile = field.path.startsWith('rom.');
            if (field.group !== group) {
                group = field.group;
                container.append(make('h3', group, 'expert-group-title'));
            }
            const row = make('div', null, 'expert-field');
            const label = make('label', field.label);
            const id = 'expert-' + field.path.replaceAll('.', '-');
            label.htmlFor = id;
            const info = infoBubble(field);
            row.append(label, info, make('code', field.key, 'expert-key'));
            let input;
            if (field.choices || field.type === 'boolean') {
                input = make('select');
                if (field.optional) {
                    const option = make('option', inheritsProfile ? 'Inherit from profile' : 'Use default');
                    option.value = '';
                    input.append(option);
                }
                const choices = field.choices || [[true, 'True'], [false, 'False']];
                for (const [value, title] of choices) {
                    const option = make('option', title);
                    option.value = String(value);
                    input.append(option);
                }
            } else if (field.type === 'array' || field.type === 'object') {
                input = make('textarea');
                input.rows = 2;
                input.spellcheck = false;
            } else {
                input = make('input');
                input.type = ['integer', 'number'].includes(field.type) ? 'number' : 'text';
                if (input.type === 'number') {
                    input.step = field.type === 'integer' ? '1' : 'any';
                    if (field.min != null) input.min = String(field.min);
                    if (field.max != null) input.max = String(field.max);
                }
            }
            input.id = id;
            input.dataset.expertPath = field.path;
            input.placeholder = field.optional ? (inheritsProfile ? 'Inherit from profile' : `Default: ${field.default ?? ''}`) : String(field.default ?? '');
            const valueRow = make('div', null, 'expert-value');
            valueRow.append(input);
            const reset = make('button', field.optional && inheritsProfile ? 'Inherit' : 'Reset', 'btn-small');
            reset.type = 'button';
            reset.title = field.optional && inheritsProfile ? 'Remove this per-game override' : 'Restore the standard value';
            valueRow.append(reset);
            const help = make('p', field.status === 'inactive' ? 'No active PC Engine effect identified.' : '', 'setting-hint expert-field-note');
            help.id = id + '-help';
            const error = make('p', '', 'expert-error');
            error.id = id + '-error';
            error.setAttribute('role', 'alert');
            input.setAttribute('aria-describedby', help.id + ' ' + error.id);
            row.append(valueRow, help, error);
            container.append(row);
            const show = () => {
                const choices = field.choicesByArch?.[current?.rom?.arch];
                info.update(choices ? { ...field, choices } : field);
                if (choices) {
                    input.replaceChildren();
                    for (const [value, title] of choices) {
                        const option = make('option', title);
                        option.value = String(value);
                        input.append(option);
                    }
                }
                const value = get(current, field.path);
                const text = value == null ? '' : typeof value === 'object' ? JSON.stringify(value, null, 2) : String(value);
                // Retain an imported value even when it is outside the usual choices.
                if (input.tagName === 'SELECT') {
                    for (const option of [...input.options]) if (option.dataset.custom) option.remove();
                    if (![...input.options].some(option => option.value === text)) {
                        const option = make('option', text === '' ? 'Not set' : `Current: ${text}`);
                        option.value = text;
                        option.dataset.custom = 'true';
                        input.append(option);
                    }
                }
                input.value = text;
                error.textContent = '';
                input.removeAttribute('aria-invalid');
                reset.disabled = field.optional && value == null;
            };
            input.addEventListener('change', () => {
                if (!current) return;
                try {
                    const value = parse(field, input.value);
                    assign(current, field, value);
                    error.textContent = '';
                    input.removeAttribute('aria-invalid');
                    reset.disabled = field.optional && value === undefined;
                    onChange(field.path);
                } catch (exception) {
                    error.textContent = exception.message;
                    input.setAttribute('aria-invalid', 'true');
                }
            });
            reset.addEventListener('click', () => {
                if (!current) return;
                assign(current, field, field.optional ? undefined : field.default);
                show();
                onChange(field.path);
            });
            controls.push(show);
        }
        const managed = make('details', null, 'expert-managed');
        managed.append(make('summary', 'Fields managed by Chronos'));
        for (const field of catalog.parameters.filter(f => f.ui === 'managed')) {
            const paragraph = make('p');
            paragraph.append(make('code', field.key), infoBubble(field), document.createTextNode(' — ' + field.help));
            managed.append(paragraph);
        }
        container.append(managed);
        return { render(game) { current = game; controls.forEach(show => show()); } };
    }
    window.ChronosExpert = { create, parse, validate, assign, infoBubble, possibleValues, decorateDetails };
})();
