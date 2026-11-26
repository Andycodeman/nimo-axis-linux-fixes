// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * nimo-wmi: NIMO AXIS 395 (N161L, built by Emdoor) laptop extras.
 *
 * - 4-zone RGB keyboard backlight: multicolor LED "nimo::kbd_backlight"
 *   (brightness 0-10, multi_intensity = R G B), plus kbd_effect, kbd_speed,
 *   kbd_zone_colors and kbd_lightbar_colors on the WMI device
 * - Windows / Fn / Print Screen key toggles: win_key, fn_key, prtsc_key
 * - Fan speeds: hwmon fan1_input, fan2_input (read straight from the EC)
 * - Battery charge limit: charge_control_end_threshold on BAT0
 * - Power modes: platform_profile (quiet / balanced / performance)
 *
 * The protocol is documented in notes/control-center-protocol.md.
 *
 * Everything except the keyboard goes straight to the EC, because the
 * firmware's other WMI methods abort on Linux with AE_AML_BUFFER_LIMIT
 * (a 63-bit field in an 8-byte buffer). The EC writes are the same ones
 * the firmware's own methods would make.
 *
 * Power modes can't be done that way: the mode is a firmware variable that
 * the firmware re-applies (with AMD power limits) on every AC plug/unplug.
 * So if the firmware's power-mode method is broken, the driver loads a copy
 * of it with the one bad field fixed (aml/wmcc-fixed.asl, as \_SB.EWMI.NPWR)
 * and calls that. On a fixed BIOS the original works and nothing is loaded.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/acpi.h>
#include <linux/delay.h>
#include <linux/dmi.h>
#include <linux/hwmon.h>
#include <linux/kmemleak.h>
#include <linux/led-class-multicolor.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/power_supply.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/sysfs.h>
#include <linux/platform_profile.h>
#include <linux/wmi.h>
#include <acpi/battery.h>

#include "wmcc-fixed.h"

#define NIMO_KBD_GUID "8600ACCE-FB9B-443E-86F4-3C867398AAE5"

/* EmdAcpi_AppKBLedControl method ids (ACPI \_SB.EWMI.WMDA) */
#define KBD_SET_4ZONE		3
#define KBD_SET_HOTKEY		7
#define KBD_GET_HOTKEY		8
#define KBD_GET_LIGHT_LEVEL	12
#define KBD_SET_SLEEP_TIMER	10

/* Set4ZoneKBLed: value 0xFE in effect/brightness/speed = leave unchanged */
#define KBD_KEEP		0xFE
#define KBD_MAX_LEVEL		10
#define KBD_ZONES		4

/* SetKBHotkey bits; a set bit means that key is turned off */
#define HOTKEY_WIN_BIT		12
#define HOTKEY_FN_BIT		13
#define HOTKEY_PRTSC_BIT	14

/* EC registers (from the DSDT's EmbeddedControl field ECF2) */
#define EC_FAN1_HI		0x76
#define EC_FAN1_LO		0x77
#define EC_FAN2_HI		0x79
#define EC_FAN2_LO		0x7A
#define EC_CHARGE_MAX		0xBB	/* MXCP: stop charging at this % */
#define EC_CHARGE_MIN		0xBC	/* MICP: start again below this % */

static bool force;
module_param(force, bool, 0444);
MODULE_PARM_DESC(force, "Load on machines other than the NIMO N161L (unsafe: EC offsets may differ)");

static const char * const kbd_effect_names[] = {
	"static",		/* 0 Always */
	"twinkle",		/* 1 */
	"wave",			/* 2 */
	"breath",		/* 3 */
	"cycle",		/* 4 colour cycle */
	"reactive",		/* 5 */
	"ripple",		/* 6 */
	"spiral_rainbow",	/* 7 */
	"rainbow_ripple",	/* 8 */
};

struct nimo_priv {
	struct wmi_device *wdev;
	struct mutex lock;	/* serialises WMI calls and the cached state */
	struct led_classdev_mc mc;
	struct mc_subled subleds[3];
	struct acpi_battery_hook battery_hook;
	struct device *ppdev;
	/* last lighting state we sent */
	u8 effect;
	u8 speed;
	u8 level;
	u8 zone_rgb[KBD_ZONES][3];
	u8 bar_rgb[2][3];	/* left, right light bar */
	u16 sleep_timer;	/* seconds, 0 = never */
	bool lit;		/* light on, as far as we know */
	bool bars_set;		/* bar_rgb has been set since load */
	bool zones_known;	/* zone_rgb is what the keyboard shows (static) */
};

/* ---- WMI ---------------------------------------------------------------- */

/*
 * Call a WMDA method with an 8-byte input. Copies up to @outlen bytes of the
 * returned buffer into @out (zero-filled if shorter).
 */
static int nimo_kbd_call(struct nimo_priv *p, u32 method, const u8 in[8],
			 u8 *out, size_t outlen)
{
	struct acpi_buffer input = { 8, (void *)in };
	struct acpi_buffer output = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;
	int ret = 0;

	lockdep_assert_held(&p->lock);

	status = wmidev_evaluate_method(p->wdev, 0, method, &input, &output);
	if (ACPI_FAILURE(status)) {
		dev_err(&p->wdev->dev, "WMI method %u failed: %s\n", method,
			acpi_format_exception(status));
		return -EIO;
	}

	obj = output.pointer;
	if (out) {
		memset(out, 0, outlen);
		if (obj && obj->type == ACPI_TYPE_BUFFER)
			memcpy(out, obj->buffer.pointer,
			       min_t(size_t, outlen, obj->buffer.length));
		else
			ret = -EPROTO;
	}
	kfree(obj);
	return ret;
}

static int nimo_kbd_set(struct nimo_priv *p, u8 zone_mask, const u8 rgb[3],
			u8 level, u8 speed, u8 effect, bool on)
{
	u8 in[8] = { zone_mask, rgb[0], rgb[1], rgb[2], level, speed, effect, on };
	u8 out[2];
	int ret;

	ret = nimo_kbd_call(p, KBD_SET_4ZONE, in, out, sizeof(out));
	if (ret)
		return ret;
	return (out[0] | out[1]) ? -EIO : 0;
}

static int nimo_kbd_off(struct nimo_priv *p)
{
	int ret = nimo_kbd_set(p, 0, p->zone_rgb[0], KBD_KEEP, KBD_KEEP,
			       KBD_KEEP, false);

	if (!ret) {
		p->lit = false;
		p->zones_known = false;
	}
	return ret;
}

/* Re-send the light bar colours, if they've been set since load. */
static int nimo_kbd_bars(struct nimo_priv *p)
{
	int i, ret = 0;

	for (i = 0; i < 2 && !ret && p->bars_set; i++) {
		if (i)
			msleep(20);
		ret = nimo_kbd_set(p, BIT(4 + i), p->bar_rgb[i], KBD_KEEP,
				   KBD_KEEP, KBD_KEEP, true);
	}
	return ret;
}

/*
 * Send the whole cached state: mode, every zone's colour (static mode) or
 * the effect, brightness, speed, and the light bars if they've been set.
 * Never use zone mask 0x0F here: that paints all four zones zone 0's colour.
 */
static int nimo_kbd_restore(struct nimo_priv *p)
{
	int i, ret;

	if (!p->level)
		return nimo_kbd_off(p);

	if (p->effect == 0) {
		/* Static mode with zone 0, then each other zone on its own. */
		ret = nimo_kbd_set(p, BIT(0), p->zone_rgb[0], p->level, p->speed,
				   0, true);
		for (i = 1; i < KBD_ZONES && !ret; i++) {
			msleep(20);
			ret = nimo_kbd_set(p, BIT(i), p->zone_rgb[i], KBD_KEEP,
					   KBD_KEEP, KBD_KEEP, true);
		}
		ret = ret ? ret : nimo_kbd_bars(p);
	} else {
		/*
		 * Bars first, effect last: a light-bar colour command sent after
		 * an effect freezes the effect after one pass.
		 */
		ret = nimo_kbd_bars(p);
		if (!ret)
			ret = nimo_kbd_set(p, 0, p->zone_rgb[0], p->level, p->speed,
					   p->effect, true);
	}
	if (!ret) {
		p->lit = true;
		p->zones_known = p->effect == 0;
	}
	return ret;
}

/* Brightness only, when the light is already on. */
static int nimo_kbd_level(struct nimo_priv *p)
{
	if (!p->level)
		return nimo_kbd_off(p);
	if (!p->lit)
		return nimo_kbd_restore(p);
	/* With an effect running, re-issue it with the new level so it keeps going. */
	return nimo_kbd_set(p, 0, p->zone_rgb[0], p->level,
			    p->effect ? p->speed : KBD_KEEP,
			    p->effect ? p->effect : KBD_KEEP, true);
}

static int nimo_hotkey_get(struct nimo_priv *p, u16 *val)
{
	u8 in[8] = { };
	u8 out[4];
	int ret;

	ret = nimo_kbd_call(p, KBD_GET_HOTKEY, in, out, sizeof(out));
	if (ret)
		return ret;
	if (out[0] | out[1])
		return -EIO;
	*val = out[2] | (out[3] << 8);
	return 0;
}

static int nimo_hotkey_set(struct nimo_priv *p, u16 val)
{
	u8 in[8] = { val & 0xFF, val >> 8 };
	u8 out[2];
	int ret;

	ret = nimo_kbd_call(p, KBD_SET_HOTKEY, in, out, sizeof(out));
	if (ret)
		return ret;
	return (out[0] | out[1]) ? -EIO : 0;
}

/* ---- keyboard backlight LED --------------------------------------------- */

static struct nimo_priv *mc_to_priv(struct led_classdev *cdev)
{
	return container_of(lcdev_to_mccdev(cdev), struct nimo_priv, mc);
}

static int nimo_led_set(struct led_classdev *cdev, enum led_brightness value)
{
	struct nimo_priv *p = mc_to_priv(cdev);
	u8 rgb[3];
	int i, ret;

	for (i = 0; i < 3; i++)
		rgb[i] = p->subleds[i].intensity;

	mutex_lock(&p->lock);
	p->level = min_t(u8, value, KBD_MAX_LEVEL);
	/* A colour written through the LED means one static colour everywhere. */
	if (memcmp(rgb, p->zone_rgb[0], 3)) {
		for (i = 0; i < KBD_ZONES; i++)
			memcpy(p->zone_rgb[i], rgb, 3);
		p->effect = 0;
		ret = nimo_kbd_restore(p);
	} else {
		ret = nimo_kbd_level(p);
	}
	mutex_unlock(&p->lock);
	return ret;
}

/* ---- sysfs on the WMI device -------------------------------------------- */

static ssize_t kbd_effect_show(struct device *dev, struct device_attribute *attr,
			       char *buf)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	int i, len = 0;

	for (i = 0; i < ARRAY_SIZE(kbd_effect_names); i++)
		len += sysfs_emit_at(buf, len, i == p->effect ? "[%s] " : "%s ",
				     kbd_effect_names[i]);
	buf[len - 1] = '\n';
	return len;
}

static ssize_t kbd_effect_store(struct device *dev, struct device_attribute *attr,
				const char *buf, size_t count)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	int effect, ret;

	effect = sysfs_match_string(kbd_effect_names, buf);
	if (effect < 0)
		return effect;

	mutex_lock(&p->lock);
	p->effect = effect;
	if (!p->level)
		p->level = KBD_MAX_LEVEL / 2;
	ret = nimo_kbd_restore(p);
	mutex_unlock(&p->lock);
	if (!ret)
		p->mc.led_cdev.brightness = p->level;
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(kbd_effect);

static ssize_t kbd_speed_show(struct device *dev, struct device_attribute *attr,
			      char *buf)
{
	struct nimo_priv *p = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%u\n", p->speed);
}

static ssize_t kbd_speed_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	u8 speed;
	int ret;

	ret = kstrtou8(buf, 10, &speed);
	if (ret)
		return ret;
	if (speed < 1 || speed > 10)
		return -EINVAL;

	mutex_lock(&p->lock);
	p->speed = speed;
	ret = p->lit ? nimo_kbd_set(p, 0, p->zone_rgb[0], KBD_KEEP, speed,
				    p->effect ? p->effect : KBD_KEEP, true) : 0;
	mutex_unlock(&p->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(kbd_speed);

static ssize_t kbd_zone_colors_show(struct device *dev,
				    struct device_attribute *attr, char *buf)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	int i, len = 0;

	for (i = 0; i < KBD_ZONES; i++)
		len += sysfs_emit_at(buf, len, "%02x%02x%02x%c", p->zone_rgb[i][0],
				     p->zone_rgb[i][1], p->zone_rgb[i][2],
				     i == KBD_ZONES - 1 ? '\n' : ' ');
	return len;
}

/* "RRGGBB RRGGBB RRGGBB RRGGBB": static colours for zones 0-3, left to right. */
static ssize_t kbd_zone_colors_store(struct device *dev,
				     struct device_attribute *attr,
				     const char *buf, size_t count)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	u8 rgb[KBD_ZONES][3];
	unsigned int c[KBD_ZONES];
	int i, ret = 0;

	if (sscanf(buf, "%6x %6x %6x %6x", &c[0], &c[1], &c[2], &c[3]) != KBD_ZONES)
		return -EINVAL;
	for (i = 0; i < KBD_ZONES; i++) {
		rgb[i][0] = c[i] >> 16;
		rgb[i][1] = c[i] >> 8;
		rgb[i][2] = c[i];
	}

	mutex_lock(&p->lock);
	if (!p->level)
		p->level = KBD_MAX_LEVEL / 2;
	if (p->lit && p->effect == 0 && p->zones_known) {
		/*
		 * Already showing static colours we set: send only the zones
		 * that changed. Each command takes ~250 ms in the firmware.
		 */
		bool first = true;

		for (i = 0; i < KBD_ZONES && !ret; i++) {
			if (!memcmp(p->zone_rgb[i], rgb[i], 3))
				continue;
			if (!first)
				msleep(20);
			first = false;
			ret = nimo_kbd_set(p, BIT(i), rgb[i], KBD_KEEP, KBD_KEEP,
					   KBD_KEEP, true);
			if (!ret)
				memcpy(p->zone_rgb[i], rgb[i], 3);
		}
	} else {
		memcpy(p->zone_rgb, rgb, sizeof(rgb));
		p->effect = 0;
		ret = nimo_kbd_restore(p);
	}
	if (!ret) {
		p->mc.led_cdev.brightness = p->level;
		for (i = 0; i < 3; i++)
			p->subleds[i].intensity = rgb[0][i];
	}
	mutex_unlock(&p->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(kbd_zone_colors);

static ssize_t kbd_lightbar_colors_show(struct device *dev,
					struct device_attribute *attr, char *buf)
{
	struct nimo_priv *p = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%02x%02x%02x %02x%02x%02x\n",
			  p->bar_rgb[0][0], p->bar_rgb[0][1], p->bar_rgb[0][2],
			  p->bar_rgb[1][0], p->bar_rgb[1][1], p->bar_rgb[1][2]);
}

/*
 * "RRGGBB RRGGBB": left and right light bar. Zone-mask bits 4 and 5 of
 * Set4ZoneKBLed (EC mailbox 0x40); effect, brightness and speed are left alone.
 */
static ssize_t kbd_lightbar_colors_store(struct device *dev,
					 struct device_attribute *attr,
					 const char *buf, size_t count)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	unsigned int c[2];
	u8 rgb[2][3];
	int i, ret = 0;

	if (sscanf(buf, "%6x %6x", &c[0], &c[1]) != 2)
		return -EINVAL;
	for (i = 0; i < 2; i++) {
		rgb[i][0] = c[i] >> 16;
		rgb[i][1] = c[i] >> 8;
		rgb[i][2] = c[i];
	}

	mutex_lock(&p->lock);
	for (i = 0; i < 2 && !ret; i++) {
		/* Skip a bar that already shows this colour (each command ~250 ms). */
		if (p->bars_set && p->lit && !memcmp(p->bar_rgb[i], rgb[i], 3))
			continue;
		if (i)
			msleep(20);
		ret = nimo_kbd_set(p, BIT(4 + i), rgb[i], KBD_KEEP, KBD_KEEP,
				   KBD_KEEP, true);
		if (!ret)
			memcpy(p->bar_rgb[i], rgb[i], 3);
	}
	if (!ret) {
		p->bars_set = true;
		/* A bar colour stops a running effect; start it again. */
		if (p->effect != 0 && p->lit)
			ret = nimo_kbd_set(p, 0, p->zone_rgb[0], KBD_KEEP, p->speed,
					   p->effect, true);
	}
	mutex_unlock(&p->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(kbd_lightbar_colors);

static ssize_t kbd_sleep_timer_show(struct device *dev,
				    struct device_attribute *attr, char *buf)
{
	struct nimo_priv *p = dev_get_drvdata(dev);

	return sysfs_emit(buf, "%u\n", p->sleep_timer);
}

/*
 * Seconds of no typing before the keyboard light turns off (0 = never).
 * Control Center offers 60, 300, 600, 900, 1200, 1800, 3600, 7200 and 0.
 * The firmware's method (WMDA 10) queues the EC command but returns an
 * integer instead of its result buffer (that's why it fails on Windows), so
 * the return value is ignored here.
 */
static ssize_t kbd_sleep_timer_store(struct device *dev,
				     struct device_attribute *attr,
				     const char *buf, size_t count)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	u16 secs;
	int ret;

	ret = kstrtou16(buf, 10, &secs);
	if (ret)
		return ret;

	mutex_lock(&p->lock);
	{
		u8 in[8] = { secs & 0xFF, secs >> 8 };

		ret = nimo_kbd_call(p, KBD_SET_SLEEP_TIMER, in, NULL, 0);
	}
	/* The method doesn't wait for the EC; give it time before the next command. */
	msleep(50);
	if (!ret)
		p->sleep_timer = secs;
	mutex_unlock(&p->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(kbd_sleep_timer);

/* win_key / fn_key / prtsc_key: 1 = on (Control Center's toggle on), 0 = off */
static ssize_t hotkey_show(struct device *dev, char *buf, int bit)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	u16 val;
	int ret;

	mutex_lock(&p->lock);
	ret = nimo_hotkey_get(p, &val);
	mutex_unlock(&p->lock);
	if (ret)
		return ret;
	return sysfs_emit(buf, "%d\n", !(val & BIT(bit)));
}

static ssize_t hotkey_store(struct device *dev, const char *buf, size_t count,
			    int bit)
{
	struct nimo_priv *p = dev_get_drvdata(dev);
	bool enable;
	u16 val;
	int ret;

	ret = kstrtobool(buf, &enable);
	if (ret)
		return ret;

	mutex_lock(&p->lock);
	ret = nimo_hotkey_get(p, &val);
	if (!ret) {
		/* Control Center only ever sends these three bits. */
		val &= BIT(HOTKEY_WIN_BIT) | BIT(HOTKEY_FN_BIT) | BIT(HOTKEY_PRTSC_BIT);
		if (enable)
			val &= ~BIT(bit);
		else
			val |= BIT(bit);
		ret = nimo_hotkey_set(p, val);
	}
	mutex_unlock(&p->lock);
	return ret ? ret : count;
}

#define NIMO_HOTKEY_ATTR(_name, _bit)						\
static ssize_t _name##_show(struct device *dev, struct device_attribute *attr,	\
			    char *buf)						\
{										\
	return hotkey_show(dev, buf, _bit);					\
}										\
static ssize_t _name##_store(struct device *dev, struct device_attribute *attr,	\
			     const char *buf, size_t count)			\
{										\
	return hotkey_store(dev, buf, count, _bit);				\
}										\
static DEVICE_ATTR_RW(_name)

NIMO_HOTKEY_ATTR(win_key, HOTKEY_WIN_BIT);
NIMO_HOTKEY_ATTR(fn_key, HOTKEY_FN_BIT);
NIMO_HOTKEY_ATTR(prtsc_key, HOTKEY_PRTSC_BIT);

static struct attribute *nimo_attrs[] = {
	&dev_attr_kbd_effect.attr,
	&dev_attr_kbd_speed.attr,
	&dev_attr_kbd_zone_colors.attr,
	&dev_attr_kbd_lightbar_colors.attr,
	&dev_attr_kbd_sleep_timer.attr,
	&dev_attr_win_key.attr,
	&dev_attr_fn_key.attr,
	&dev_attr_prtsc_key.attr,
	NULL
};
ATTRIBUTE_GROUPS(nimo);

/* ---- fans (hwmon, EC read only) ----------------------------------------- */

static int nimo_ec_read16(u8 hi_reg, u8 lo_reg, long *val)
{
	u8 hi, lo;
	int ret;

	ret = ec_read(hi_reg, &hi);
	if (!ret)
		ret = ec_read(lo_reg, &lo);
	if (ret)
		return ret;
	*val = (hi << 8) | lo;
	return 0;
}

static umode_t nimo_hwmon_is_visible(const void *data,
				     enum hwmon_sensor_types type, u32 attr,
				     int channel)
{
	return type == hwmon_fan && attr == hwmon_fan_input ? 0444 : 0;
}

static int nimo_hwmon_read(struct device *dev, enum hwmon_sensor_types type,
			   u32 attr, int channel, long *val)
{
	if (type != hwmon_fan || attr != hwmon_fan_input)
		return -EOPNOTSUPP;
	return channel == 0 ? nimo_ec_read16(EC_FAN1_HI, EC_FAN1_LO, val)
			    : nimo_ec_read16(EC_FAN2_HI, EC_FAN2_LO, val);
}

static const struct hwmon_ops nimo_hwmon_ops = {
	.is_visible = nimo_hwmon_is_visible,
	.read = nimo_hwmon_read,
};

static const struct hwmon_channel_info * const nimo_hwmon_info[] = {
	HWMON_CHANNEL_INFO(fan, HWMON_F_INPUT, HWMON_F_INPUT),
	NULL
};

static const struct hwmon_chip_info nimo_hwmon_chip = {
	.ops = &nimo_hwmon_ops,
	.info = nimo_hwmon_info,
};

/* ---- battery charge limit (EC) ------------------------------------------ */

static ssize_t charge_control_end_threshold_show(struct device *dev,
						 struct device_attribute *attr,
						 char *buf)
{
	u8 max;
	int ret;

	ret = ec_read(EC_CHARGE_MAX, &max);
	if (ret)
		return ret;
	/* 0 = no limit */
	return sysfs_emit(buf, "%u\n", (max == 0 || max > 100) ? 100 : max);
}

static ssize_t charge_control_end_threshold_store(struct device *dev,
						  struct device_attribute *attr,
						  const char *buf, size_t count)
{
	u8 val;
	int ret;

	ret = kstrtou8(buf, 10, &val);
	if (ret)
		return ret;
	if (val < 20 || val > 100)
		return -EINVAL;
	if (val == 100)
		val = 0;	/* firmware default: no limit */

	/* Same writes as the firmware's SetBatteryChargeRationing (WMDD 2). */
	ret = ec_write(EC_CHARGE_MAX, val);
	if (!ret)
		ret = ec_write(EC_CHARGE_MIN, val ? val - 1 : 0);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(charge_control_end_threshold);

static struct attribute *nimo_battery_attrs[] = {
	&dev_attr_charge_control_end_threshold.attr,
	NULL
};
ATTRIBUTE_GROUPS(nimo_battery);

static int nimo_battery_add(struct power_supply *battery,
			    struct acpi_battery_hook *hook)
{
	return device_add_groups(&battery->dev, nimo_battery_groups);
}

static int nimo_battery_remove(struct power_supply *battery,
			       struct acpi_battery_hook *hook)
{
	device_remove_groups(&battery->dev, nimo_battery_groups);
	return 0;
}

/* ---- power modes (platform_profile) ------------------------------------ */

#define WMCC_PATH	"\\_SB.EWMI.WMCC"	/* NIMO's own method */
#define NPWR_PATH	"\\_SB.EWMI.NPWR"	/* our fixed copy, aml/wmcc-fixed.asl */
#define WMCC_SET	1
#define WMCC_GET	2

enum nimo_power_mode {
	NIMO_MODE_QUIET = 1,
	NIMO_MODE_BALANCED = 2,
	NIMO_MODE_PERFORMANCE = 3,
};

/* Whichever of WMCC_PATH / NPWR_PATH works; set once by nimo_power_init(). */
static const char *nimo_power_path;

/* Call the power-mode method; returns the mode the firmware reports. */
static acpi_status nimo_power_call(const char *path, u32 func, u32 mode,
				   u16 *out_mode)
{
	union acpi_object args[3] = {
		{ .integer = { ACPI_TYPE_INTEGER, 0 } },
		{ .integer = { ACPI_TYPE_INTEGER, func } },
		{ .integer = { ACPI_TYPE_INTEGER, mode } },
	};
	struct acpi_object_list arg_list = { ARRAY_SIZE(args), args };
	struct acpi_buffer output = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *obj;
	acpi_status status;

	status = acpi_evaluate_object(NULL, (acpi_string)path, &arg_list, &output);
	if (ACPI_FAILURE(status))
		return status;

	obj = output.pointer;
	if (!obj || obj->type != ACPI_TYPE_BUFFER || obj->buffer.length < 4) {
		status = AE_TYPE;
	} else {
		u8 *b = obj->buffer.pointer;

		if (b[0] | b[1])	/* status word: 0 = success */
			status = AE_ERROR;
		else if (out_mode)
			*out_mode = b[2] | (b[3] << 8);
	}
	kfree(obj);
	return status;
}

/*
 * Pick a working power-mode method. If NIMO's WMCC hits the firmware bug,
 * load aml/wmcc-fixed.asl as an SSDT, which adds \_SB.EWMI.NPWR.
 *
 * ACPICA keeps using the table memory after acpi_load_table(), so the copy is
 * never freed. The table stays loaded until reboot, and a reloaded driver finds
 * NPWR already there. (acpi_install_method() can't be used to replace WMCC in
 * place: on Ubuntu's 7.0.0-44 kernel it rejects every buffer with
 * AE_BAD_PARAMETER, because its opcode check reads an aml_end it never sets.)
 */
static int nimo_power_init(struct device *dev)
{
	struct acpi_table_header *table;
	acpi_status status;
	u16 mode;

	if (acpi_has_method(NULL, NPWR_PATH))
		goto use_npwr;

	status = nimo_power_call(WMCC_PATH, WMCC_GET, 0, &mode);
	if (ACPI_SUCCESS(status)) {
		nimo_power_path = WMCC_PATH;	/* fixed BIOS: use NIMO's method */
		return 0;
	}
	if (status != AE_AML_BUFFER_LIMIT) {
		dev_warn(dev, "power-mode method unavailable (%s)\n",
			 acpi_format_exception(status));
		return -ENODEV;
	}

	dev_info(dev, "firmware power-mode method hits the BIOS bug (AE_AML_BUFFER_LIMIT); loading the fixed copy\n");
	table = kmemdup(nimo_npwr_ssdt, sizeof(nimo_npwr_ssdt), GFP_KERNEL);
	if (!table)
		return -ENOMEM;
	status = acpi_load_table(table, NULL);
	if (ACPI_FAILURE(status)) {
		kfree(table);
		dev_warn(dev, "loading the fixed power-mode method failed: %s\n",
			 acpi_format_exception(status));
		return -ENODEV;
	}
	kmemleak_not_leak(table);

use_npwr:
	status = nimo_power_call(NPWR_PATH, WMCC_GET, 0, &mode);
	if (ACPI_FAILURE(status)) {
		dev_warn(dev, "fixed power-mode method doesn't work: %s\n",
			 acpi_format_exception(status));
		return -ENODEV;
	}
	nimo_power_path = NPWR_PATH;
	return 0;
}

static int nimo_profile_probe(void *drvdata, unsigned long *choices)
{
	set_bit(PLATFORM_PROFILE_QUIET, choices);
	set_bit(PLATFORM_PROFILE_BALANCED, choices);
	set_bit(PLATFORM_PROFILE_PERFORMANCE, choices);
	return 0;
}

static int nimo_profile_get(struct device *dev,
			    enum platform_profile_option *profile)
{
	u16 mode;

	if (ACPI_FAILURE(nimo_power_call(nimo_power_path, WMCC_GET, 0, &mode)))
		return -EIO;
	switch (mode) {
	case NIMO_MODE_QUIET:
		*profile = PLATFORM_PROFILE_QUIET;
		break;
	case NIMO_MODE_BALANCED:
		*profile = PLATFORM_PROFILE_BALANCED;
		break;
	case NIMO_MODE_PERFORMANCE:
		*profile = PLATFORM_PROFILE_PERFORMANCE;
		break;
	default:
		return -EINVAL;
	}
	return 0;
}

static int nimo_profile_set(struct device *dev,
			    enum platform_profile_option profile)
{
	u32 mode;
	u16 now;

	switch (profile) {
	case PLATFORM_PROFILE_QUIET:
		mode = NIMO_MODE_QUIET;
		break;
	case PLATFORM_PROFILE_BALANCED:
		mode = NIMO_MODE_BALANCED;
		break;
	case PLATFORM_PROFILE_PERFORMANCE:
		mode = NIMO_MODE_PERFORMANCE;
		break;
	default:
		return -EOPNOTSUPP;
	}
	if (ACPI_FAILURE(nimo_power_call(nimo_power_path, WMCC_SET, mode, &now)) ||
	    now != mode)
		return -EIO;
	return 0;
}

static const struct platform_profile_ops nimo_profile_ops = {
	.probe = nimo_profile_probe,
	.profile_get = nimo_profile_get,
	.profile_set = nimo_profile_set,
};

/* ---- probe -------------------------------------------------------------- */

static const struct dmi_system_id nimo_dmi_table[] = {
	{
		.matches = {
			DMI_MATCH(DMI_SYS_VENDOR, "Nimo"),
			DMI_MATCH(DMI_PRODUCT_NAME, "N161L"),
		},
	},
	{ }
};

static int nimo_probe(struct wmi_device *wdev, const void *context)
{
	struct device *dev = &wdev->dev;
	struct nimo_priv *p;
	struct device *hwmon;
	u8 in[8] = { };
	u8 level[1];
	int i, ret;

	if (!dmi_check_system(nimo_dmi_table) && !force) {
		dev_info(dev, "not a NIMO N161L; use force=1 to load anyway\n");
		return -ENODEV;
	}

	p = devm_kzalloc(dev, sizeof(*p), GFP_KERNEL);
	if (!p)
		return -ENOMEM;
	p->wdev = wdev;
	ret = devm_mutex_init(dev, &p->lock);
	if (ret)
		return ret;
	dev_set_drvdata(dev, p);

	/* Starting state: what Control Center's defaults would send. */
	p->speed = 5;
	p->effect = 0;
	p->lit = true;	/* the firmware keeps the light on across boots */
	for (i = 0; i < KBD_ZONES; i++) {
		p->zone_rgb[i][0] = 0x00;
		p->zone_rgb[i][1] = 0xFF;
		p->zone_rgb[i][2] = 0xFF;	/* cyan, the app's default */
	}
	/*
	 * GetKBLightLevel/GetKBLightState (EC 0x58/0x59) follow the Fn+F8/F9
	 * keys, not Set4ZoneKBLed, so they can't be used to read our state
	 * back. Start from the hardware level and keep our own copy.
	 */
	mutex_lock(&p->lock);
	ret = nimo_kbd_call(p, KBD_GET_LIGHT_LEVEL, in, level, 1);
	mutex_unlock(&p->lock);
	p->level = ret ? KBD_MAX_LEVEL / 2 : min_t(u8, level[0], KBD_MAX_LEVEL);

	p->subleds[0] = (struct mc_subled){ .color_index = LED_COLOR_ID_RED,
					    .intensity = p->zone_rgb[0][0] };
	p->subleds[1] = (struct mc_subled){ .color_index = LED_COLOR_ID_GREEN,
					    .intensity = p->zone_rgb[0][1] };
	p->subleds[2] = (struct mc_subled){ .color_index = LED_COLOR_ID_BLUE,
					    .intensity = p->zone_rgb[0][2] };
	p->mc.subled_info = p->subleds;
	p->mc.num_colors = 3;
	p->mc.led_cdev.name = "nimo::kbd_backlight";
	p->mc.led_cdev.max_brightness = KBD_MAX_LEVEL;
	p->mc.led_cdev.brightness = p->level;
	p->mc.led_cdev.brightness_set_blocking = nimo_led_set;
	ret = devm_led_classdev_multicolor_register(dev, &p->mc);
	if (ret)
		return ret;

	hwmon = devm_hwmon_device_register_with_info(dev, "nimo", p,
						     &nimo_hwmon_chip, NULL);
	if (IS_ERR(hwmon))
		return PTR_ERR(hwmon);

	p->battery_hook.name = "NIMO charge limit";
	p->battery_hook.add_battery = nimo_battery_add;
	p->battery_hook.remove_battery = nimo_battery_remove;
	ret = devm_battery_hook_register(dev, &p->battery_hook);
	if (ret)
		return ret;

	/* Power modes are optional: keep the rest working if they can't be set up. */
	if (!nimo_power_init(dev)) {
		p->ppdev = devm_platform_profile_register(dev, "nimo-wmi", p,
							  &nimo_profile_ops);
		if (IS_ERR(p->ppdev))
			dev_warn(dev, "platform_profile registration failed: %ld\n",
				 PTR_ERR(p->ppdev));
	}
	return 0;
}

static const struct wmi_device_id nimo_wmi_id_table[] = {
	{ NIMO_KBD_GUID, NULL },
	{ }
};
MODULE_DEVICE_TABLE(wmi, nimo_wmi_id_table);

static struct wmi_driver nimo_wmi_driver = {
	.driver = {
		.name = "nimo-wmi",
		.dev_groups = nimo_groups,
	},
	.id_table = nimo_wmi_id_table,
	.probe = nimo_probe,
	.no_singleton = true,
};
module_wmi_driver(nimo_wmi_driver);

MODULE_AUTHOR("Andycodeman");
MODULE_DESCRIPTION("NIMO AXIS 395 (N161L) keyboard RGB, hotkey toggles, fans, charge limit and power modes");
MODULE_VERSION("0.6");
MODULE_LICENSE("GPL");
