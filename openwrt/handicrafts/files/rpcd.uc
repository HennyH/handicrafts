// The backend of Services > Handicrafts in LuCI, over ubus: an rpcd ucode
// plugin installed as /usr/share/rpcd/ucode/handicrafts, exposing the
// object `handicrafts`. Modelled on Monokulo's.
//   status         whether the shop runs, its build, why it didn't start,
//                  and which secrets are set (never their values)
//   check          the settings as the next start would take them
//   set_secret     {name, value}: one of the secret files
//   clear_secret   {name}
//   set_password   {password}: hash it and keep only the hash
//   test_mail      {to}: send a test email now
//   restart
// LuCI can't read the root-only secrets folder, nor run a command, without
// this; its ACL grants the page these methods alone.

'use strict';

import { readfile, writefile, rename, unlink, chmod, access, popen } from 'fs';
import { connect } from 'ubus';

const name = 'handicrafts';
const secrets_dir = '/etc/handicrafts/secrets';
// Set through set_secret; the password's hash only through set_password.
const SECRETS = [ 'HANDICRAFTS_SMTP_PASSWORD', 'HANDICRAFTS_TUTA_PASSWORD', 'HANDICRAFTS_MONOKULO_SECRET_KEY', 'HANDICRAFTS_MONOKULO_WEBHOOK_SECRET' ];
const HASH = 'HANDICRAFTS_ADMIN_PASSWORD_HASH';

function shell_quote(s) {
	return "'" + replace(s, "'", "'\\''") + "'";
}

function run(command) {
	const proc = popen(`${command} 2>&1`);
	if (!proc)
		return { output: '', code: -1 };
	const output = proc.read('all') ?? '';
	return { output: trim(output), code: proc.close() };
}

function build(text) {
	const found = {};
	for (let line in split(text ?? '', '\n')) {
		const kv = match(line, /^(version|commit)=(.*)$/);
		if (kv)
			found[kv[1]] = trim(kv[2]);
	}
	return found;
}

function write_secret(secret, value) {
	const path = `${secrets_dir}/${secret}`;
	if (!access(secrets_dir))
		system(`mkdir -p ${secrets_dir} && chmod 700 /etc/handicrafts ${secrets_dir}`);
	if (writefile(`${path}.new`, value) == null || !chmod(`${path}.new`, 0o600) || !rename(`${path}.new`, path)) {
		unlink(`${path}.new`);
		return { ok: false, error: `Couldn't write ${path}.` };
	}
	return { ok: true };
}

const methods = {
	status: {
		call: function() {
			const ubus = connect();
			const listed = ubus?.call('service', 'list', { name });
			const instance = listed?.[name]?.instances?.[name];
			const running = instance?.running == true;
			const secrets = {};
			for (let s in [ ...SECRETS, HASH ])
				secrets[s] = length(readfile(`${secrets_dir}/${s}`) ?? '') > 0;
			const check = readfile('/tmp/handicrafts.check');
			return {
				running,
				running_build: running ? { version: instance.data?.version, commit: instance.data?.commit } : null,
				installed: build(readfile('/usr/share/handicrafts/version')),
				check_error: check == null ? null : trim(check),
				secrets
			};
		}
	},

	check: {
		call: function() {
			const checked = run('/etc/init.d/handicrafts check');
			return { ok: checked.code == 0, output: checked.output };
		}
	},

	set_secret: {
		args: { name: 'name', value: 'value' },
		call: function(request) {
			const secret = request.args.name, value = request.args.value;
			if (type(secret) != 'string' || index(SECRETS, secret) < 0)
				return { ok: false, error: 'Not a secret LuCI sets.' };
			if (type(value) != 'string' || length(trim(value)) == 0)
				return { ok: false, error: 'The value is empty: clear it instead.' };
			if (length(value) > 4096 || index(value, '\0') >= 0 || index(value, '\n') >= 0)
				return { ok: false, error: 'That is not a value the shop takes.' };
			return write_secret(secret, trim(value));
		}
	},

	clear_secret: {
		args: { name: 'name' },
		call: function(request) {
			const secret = request.args.name;
			if (type(secret) != 'string' || index(SECRETS, secret) < 0)
				return { ok: false, error: 'Not a secret LuCI clears.' };
			const path = `${secrets_dir}/${secret}`;
			if (access(path) && !unlink(path))
				return { ok: false, error: `Couldn't remove ${path}.` };
			return { ok: true };
		}
	},

	set_password: {
		args: { password: 'password' },
		call: function(request) {
			const password = request.args.password;
			if (type(password) != 'string' || length(password) < 8)
				return { ok: false, error: 'Use at least 8 characters.' };
			if (length(password) > 256 || index(password, '\n') >= 0 || index(password, '\0') >= 0)
				return { ok: false, error: 'That password can\'t be used.' };
			if (!access(secrets_dir))
				system(`mkdir -p ${secrets_dir} && chmod 700 /etc/handicrafts ${secrets_dir}`);
			// The password goes to the binary on stdin, never a command line
			// or a file; the hash lands in the root-only secrets folder.
			const out = `${secrets_dir}/${HASH}.new`;
			const proc = popen(`umask 077; /usr/bin/handicrafts hash-password > ${out} 2>&1`, 'w');
			if (!proc)
				return { ok: false, error: 'Couldn\'t hash the password.' };
			proc.write(password + '\n');
			const code = proc.close();
			const hashed = trim(readfile(out) ?? '');
			if (code != 0 || !match(hashed, /^\$argon2/)) {
				unlink(out);
				return { ok: false, error: hashed || 'Couldn\'t hash the password.' };
			}
			unlink(out);
			return write_secret(HASH, hashed);
		}
	},

	test_mail: {
		args: { to: 'to' },
		call: function(request) {
			const to = request.args.to;
			// ucode regexes are POSIX: inside [...] \s is a backslash and an
			// "s", so the class must be spelled [:space:].
			if (type(to) != 'string' || !match(to, /^[^[:space:]@'"]+@[^[:space:]@'"]+\.[^[:space:]@'"]+$/))
				return { ok: false, output: 'That isn\'t an email address.' };
			const sent = run(`/etc/init.d/handicrafts test_mail ${shell_quote(to)}`);
			return { ok: sent.code == 0, output: sent.output };
		}
	},

	restart: {
		call: function() {
			return { result: system('/etc/init.d/handicrafts restart >/dev/null 2>&1') == 0 };
		}
	}
};

return { [name]: methods };
