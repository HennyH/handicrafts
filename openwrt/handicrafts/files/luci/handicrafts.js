'use strict';
'require view';
'require form';
'require rpc';
'require uci';
'require ui';

// Services > Handicrafts: the shop's settings. Secrets (the admin password,
// mail passwords, Monokulo keys) never go into UCI: they are written to
// root-only files through the package's rpcd backend, and never shown again.

var callStatus = rpc.declare({ object: 'handicrafts', method: 'status', expect: { '': {} } });
var callCheck = rpc.declare({ object: 'handicrafts', method: 'check', expect: { '': {} } });
var callSetSecret = rpc.declare({ object: 'handicrafts', method: 'set_secret', params: [ 'name', 'value' ], expect: { '': {} } });
var callClearSecret = rpc.declare({ object: 'handicrafts', method: 'clear_secret', params: [ 'name' ], expect: { '': {} } });
var callSetPassword = rpc.declare({ object: 'handicrafts', method: 'set_password', params: [ 'password' ], expect: { '': {} } });
var callTestMail = rpc.declare({ object: 'handicrafts', method: 'test_mail', params: [ 'to' ], expect: { '': {} } });
var callRestart = rpc.declare({ object: 'handicrafts', method: 'restart', expect: { result: false } });

// Secrets changed in this save: the shop restarts to take them.
var secretsChanged = false;

function describe(b) {
	var text = '<code>' + (b.version || '?') + '</code>';
	if (/^[0-9a-f]{40}$/.test(b.commit || ''))
		text += ' &middot; ' + _('commit') + ' <code>' + b.commit.substring(0, 12) + '</code>';
	return text;
}

function notify(ok, text) {
	ui.addNotification(null, E('p', text), ok ? 'info' : 'danger');
}

// A password-like field that writes a secret file instead of UCI. Blank
// keeps what's there.
function secretOption(s, tab, secret, title, help, status) {
	var o = s.taboption(tab, form.Value, '_' + secret, title, help);
	o.password = true;
	o.rmempty = true;
	o.placeholder = status.secrets && status.secrets[secret] ? _('Set (leave blank to keep it)') : _('Not set');
	o.cfgvalue = function() { return ''; };
	o.write = function(sid, value) {
		if (!value)
			return;
		return callSetSecret(secret, value).then(function(r) {
			if (!r.ok)
				throw new Error(r.error || _('Couldn’t save it'));
			secretsChanged = true;
		});
	};
	o.remove = function() {};

	if (status.secrets && status.secrets[secret]) {
		var b = s.taboption(tab, form.Button, '_clear_' + secret, ' ');
		b.inputtitle = _('Forget %s').format(title.toLowerCase());
		b.inputstyle = 'remove';
		b.onclick = function() {
			return callClearSecret(secret).then(function(r) {
				if (!r.ok)
					return notify(false, r.error);
				return callRestart().then(function() { window.location.reload(); });
			});
		};
	}
	return o;
}

return view.extend({
	load: function() {
		return Promise.all([ uci.load('handicrafts'), L.resolveDefault(callStatus(), {}) ]);
	},

	handleSaveApply: function(ev, mode) {
		return this.handleSave(ev).then(function() {
			if (secretsChanged)
				return callRestart();
		}).then(L.bind(function() {
			secretsChanged = false;
			return ui.changes.apply(mode == '0');
		}, this));
	},

	render: function(data) {
		var st = data[1] || {};
		var m, s, o;

		m = new form.Map('handicrafts', _('Handicrafts shop'),
			_('Rachel’s Handicrafts: the shop, its orders and payments. Orders are managed at <code>/admin</code> on the shop’s own address. Save & Apply restarts the shop with the new settings.'));

		s = m.section(form.NamedSection, 'main', 'handicrafts');
		s.addremove = false;
		s.tab('general', _('General'));
		s.tab('mail', _('Email'));
		s.tab('payments', _('Payments'));
		s.tab('limits', _('Spam limits'));

		// ------------------------------------------------------------ general
		o = s.taboption('general', form.DummyValue, '_status', _('Status'));
		o.rawhtml = true;
		o.cfgvalue = function() {
			var text = '<strong>' + (st.running ? _('Running') : _('Not running')) + '</strong>';
			var b = st.running_build || st.installed || {};
			if (b.version)
				text += ' &middot; ' + describe(b);
			if (!st.running && st.check_error)
				text += '<br><span style="color:#c0392b">' + _('Not started: %s').format(st.check_error) + '</span>';
			if (st.secrets && !st.secrets.HANDICRAFTS_ADMIN_PASSWORD_HASH)
				text += '<br>' + _('/admin is closed until an admin password is set below.');
			var url = uci.get('handicrafts', 'main', 'public_url');
			if (url)
				text += '<br><a href="' + url + '/admin" target="_blank" rel="noreferrer">' + _('Open the orders page') + '</a>';
			return text;
		};

		o = s.taboption('general', form.Button, '_restart', _('Restart'));
		o.inputtitle = _('Restart the shop');
		o.inputstyle = 'reload';
		o.onclick = function() {
			return callRestart().then(function(ok) {
				if (!ok) {
					return callCheck().then(function(r) { notify(false, _('Couldn’t start: %s').format(r.output || '?')); });
				}
				window.location.reload();
			});
		};

		o = s.taboption('general', form.Flag, 'enabled', _('Enabled'));
		o.default = '1';
		o.rmempty = false;

		o = s.taboption('general', form.Value, 'public_url', _('Shop address'),
			_('The address customers use, behind the reverse proxy. Pay links in invoices start with it.'));
		o.placeholder = 'https://www.rachelshandicrafts.com';
		o.validate = function(sid, v) {
			return (!v || /^https?:\/\/[^\s]+$/.test(v)) ? true : _('A web address starting with https://');
		};

		o = s.taboption('general', form.ListValue, 'listen', _('Listen on'),
			_('With the reverse proxy on this router, keep “This router only”.'));
		o.value('127.0.0.1', _('This router only (127.0.0.1)'));
		o.value('0.0.0.0', _('All interfaces'));
		o.default = '127.0.0.1';

		o = s.taboption('general', form.Value, 'port', _('Port'));
		o.datatype = 'port';
		o.placeholder = '8082';

		o = s.taboption('general', form.DynamicList, 'trusted_proxies', _('Trusted proxies'),
			_('Only these addresses’ <code>X-Forwarded-For</code> header is believed, for the customer’s real IP in the spam limits.'));
		o.datatype = 'ipaddr';

		o = s.taboption('general', form.Value, 'admin_user', _('Admin username'));
		o.placeholder = 'rachel';
		o.rmempty = false;

		o = s.taboption('general', form.Value, '_password', _('Admin password'),
			_('For <code>/admin</code>. Hashed with argon2 here and only the hash is kept. At least 8 characters.') + ' ' +
			(st.secrets && st.secrets.HANDICRAFTS_ADMIN_PASSWORD_HASH ? _('A password is set.') : '<strong>' + _('No password is set yet.') + '</strong>'));
		o.password = true;
		o.placeholder = _('New password');
		o.cfgvalue = function() { return ''; };
		o.remove = function() {};
		o.validate = function(sid, v) { return (!v || v.length >= 8) ? true : _('At least 8 characters.'); };
		o.write = function(sid, v) {
			if (!v)
				return;
			return callSetPassword(v).then(function(r) {
				if (!r.ok)
					throw new Error(r.error || _('Couldn’t set the password'));
				secretsChanged = true;
			});
		};

		o = s.taboption('general', form.Value, 'admin_email', _('Admin email'),
			_('Where new orders, “I’ve paid” presses and Monero payments are sent.'));
		o.datatype = 'email';

		o = s.taboption('general', form.Value, 'data_dir', _('Data folder'),
			_('The database. Must be on storage, not /tmp. Kept across firmware upgrades if left at the default.'));
		o.placeholder = '/srv/handicrafts';
		o.validate = function(sid, v) {
			if (!v) return true;
			if (!/^\/[^\s]*$/.test(v)) return _('Use an absolute path such as /srv/handicrafts.');
			if (/^\/(tmp|var)(\/|$)/.test(v)) return _('/tmp and /var are in memory: everything there is lost on reboot.');
			return true;
		};

		// --------------------------------------------------------------- mail
		o = s.taboption('mail', form.ListValue, '_transport', _('Send email'));
		o.uciconfig = 'handicrafts';
		o.ucisection = 'mail';
		o.ucioption = 'transport';
		o.value('off', _('Off (emails wait, unsent)'));
		o.value('tuta', _('From a Tuta account'));
		o.value('smtp', _('Over SMTP'));
		o.value('log', _('Write to the system log (testing)'));
		o.default = 'off';

		function mailOpt(type, opt, title, help, dep) {
			var x = s.taboption('mail', type, '_' + opt, title, help);
			x.uciconfig = 'handicrafts';
			x.ucisection = 'mail';
			x.ucioption = opt;
			if (dep)
				x.depends('_transport', dep);
			return x;
		}

		o = mailOpt(form.Value, 'from', _('Send from'),
			_('The address customers see mail from. Tuta: the account’s own address (leave empty) or one of its aliases; the shop refuses to start with an address the account can’t send from. SMTP: empty uses the username.'));
		o.datatype = 'email';
		o.depends('_transport', 'tuta');
		o.depends('_transport', 'smtp');

		o = mailOpt(form.Value, 'from_name', _('From name'), _('Tuta sends with the name set in Tuta’s own settings.'), 'smtp');
		o.placeholder = 'Rachel’s Handicrafts';

		o = mailOpt(form.Value, 'tuta_user', _('Tuta address'),
			_('Tuta has no SMTP, so the shop signs in to this account with Tern (Tuta’s own SDK) and sends from it. Accounts with two-factor sign-in can’t be used: make a separate sending account without it.'), 'tuta');
		o.datatype = 'email';
		o = secretOption(s, 'mail', 'HANDICRAFTS_TUTA_PASSWORD', _('Tuta password'), null, st);
		o.depends('_transport', 'tuta');

		o = mailOpt(form.Value, 'smtp_host', _('SMTP server'), null, 'smtp');
		o.datatype = 'host';
		o = mailOpt(form.ListValue, 'smtp_security', _('Security'), null, 'smtp');
		o.value('tls', 'TLS (465)');
		o.value('starttls', 'STARTTLS (587)');
		o.value('none', _('None (25)'));
		o = mailOpt(form.Value, 'smtp_port', _('Port'), _('Empty: the usual port for the security above.'), 'smtp');
		o.datatype = 'port';
		o = mailOpt(form.Value, 'smtp_user', _('Username'), null, 'smtp');
		o = secretOption(s, 'mail', 'HANDICRAFTS_SMTP_PASSWORD', _('SMTP password'), null, st);
		o.depends('_transport', 'smtp');

		o = s.taboption('mail', form.Button, '_test', _('Test'),
			_('Sends a test email to the admin address with the settings as saved. Save & Apply first.'));
		o.inputtitle = _('Send test email');
		o.onclick = function() {
			var to = uci.get('handicrafts', 'main', 'admin_email');
			if (!to)
				return notify(false, _('Set the admin email first.'));
			return callTestMail(to).then(function(r) { notify(r.ok, r.output || (r.ok ? _('Sent.') : _('Failed.'))); });
		};

		// ----------------------------------------------------------- payments
		function payOpt(type, opt, title, help) {
			var x = s.taboption('payments', type, '_' + opt, title, help);
			x.uciconfig = 'handicrafts';
			x.ucisection = 'payments';
			x.ucioption = opt;
			return x;
		}

		o = s.taboption('payments', form.DummyValue, '_bank_head', _('Bank transfer'));
		o.rawhtml = true;
		o.cfgvalue = function() { return _('Shown on the pay page with each order’s short reference.'); };
		payOpt(form.Value, 'payid', _('PayID'));
		payOpt(form.Value, 'bank_account_name', _('Account name'));
		o = payOpt(form.Value, 'bsb', _('BSB'));
		o.validate = function(sid, v) { return (!v || /^\d{3}-?\d{3}$/.test(v)) ? true : _('Six digits, like 062-000'); };
		o = payOpt(form.Value, 'account_number', _('Account number'));
		o.validate = function(sid, v) { return (!v || /^\d{4,10}$/.test(v)) ? true : _('Digits only'); };

		o = s.taboption('payments', form.DummyValue, '_mk_head', _('Monero (Monokulo)'));
		o.rawhtml = true;
		o.cfgvalue = function() {
			var url = uci.get('handicrafts', 'main', 'public_url') || 'https://your-shop';
			return _('Set the Monokulo address, Save & Apply, then connect the store on the shop’s own <a href="%s/admin/monokulo" target="_blank" rel="noreferrer">Monero page</a>: you sign in to Monokulo and confirm, and the shop fetches the keys and adds its webhook by itself.').format(url);
		};
		o = payOpt(form.Value, 'monokulo_url', _('Monokulo address'));
		o.placeholder = 'https://www.monokulo.com';

		o = s.taboption('payments', form.DummyValue, '_mk_keys_head', _('Keys by hand'));
		o.rawhtml = true;
		o.cfgvalue = function() {
			var url = uci.get('handicrafts', 'main', 'public_url') || 'https://your-shop';
			return _('Only if you’d rather not connect: make a secret key in Monokulo, add a webhook pointing at <code>%s/webhooks/monokulo</code>, and paste all three below. A store connected on the Monero page takes over from these.').format(url);
		};
		o = payOpt(form.Value, 'monokulo_public_key', _('Public key'), _('<code>pk_…</code>: lets the embedded checkout follow the payment.'));
		o.validate = function(sid, v) { return (!v || /^pk_/.test(v)) ? true : _('Starts with pk_'); };
		secretOption(s, 'payments', 'HANDICRAFTS_MONOKULO_SECRET_KEY', _('Secret key'),
			_('<code>sk_…</code>: the shop makes each Monokulo order with it, so customers can’t change the price.'), st);
		secretOption(s, 'payments', 'HANDICRAFTS_MONOKULO_WEBHOOK_SECRET', _('Webhook signing secret'),
			_('Proves a payment notice really came from Monokulo.'), st);

		// ------------------------------------------------------------- limits
		function limit(opt, title, help, dflt) {
			var x = s.taboption('limits', form.Value, '_' + opt, title, help);
			x.uciconfig = 'handicrafts';
			x.ucisection = 'limits';
			x.ucioption = opt;
			x.datatype = 'uinteger';
			x.placeholder = dflt;
			return x;
		}
		limit('ip_10min', _('Orders per IP in 10 minutes'), null, '3');
		limit('ip_day', _('Orders per IP in a day'), null, '10');
		limit('email_day', _('Order emails per address in a day'), _('After this the order is still taken; only the customer’s copy is skipped.'), '3');
		limit('shop_day', _('Orders per day, whole shop'), _('A cap against fake orders from many addresses. You’re emailed when it’s reached.'), '40');

		return m.render();
	}
});
