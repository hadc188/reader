(function (g) {
  function native(op, data) {
    var result = JSON.parse(g.__binary(op, JSON.stringify(data)));
    if (result.error) throw new Error(result.error);
    return result.value;
  }
  /// 把任意值转成「字节数组」。
///
/// **字符串要按 UTF-8 拆字节**，不能走 `Array.prototype.slice.call` —— 那得到的是
/// 字符（`"key"` → `["k","e","y"]`），`Number("k")` 是 `NaN`，`NaN << 24` 归零，
/// 于是 `new SecretKeySpec("key", alg)` 的密钥变成 `[0,0,0]`，HMAC 全错。
/// 这与 Java 的 `String.getBytes()` 语义一致。
function stringBytes(text) {
  var out = [];
  for (var i = 0; i < text.length; i++) {
    var code = text.charCodeAt(i);
    // 代理对（U+10000 以上）合并成一个码位后再编码。
    if (code >= 0xD800 && code <= 0xDBFF && i + 1 < text.length) {
      var low = text.charCodeAt(i + 1);
      if (low >= 0xDC00 && low <= 0xDFFF) {
        code = ((code - 0xD800) << 10) + (low - 0xDC00) + 0x10000;
        i++;
      }
    }
    if (code < 0x80) out.push(code);
    else if (code < 0x800) out.push(0xC0 | (code >> 6), 0x80 | (code & 63));
    else if (code < 0x10000) out.push(0xE0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    else out.push(0xF0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
  }
  return out;
}
function array(value) {
  if (typeof value === 'string') return stringBytes(value);
  if (Array.isArray(value)) return value.map(function (v) { return (Number(v) << 24) >> 24; });
  if (ArrayBuffer.isView(value)) {
    return Array.prototype.slice.call(value).map(function (v) { return (Number(v) << 24) >> 24; });
  }
  if (value && typeof value.read === 'function') {
    var out = [], one;
    while ((one = value.read()) !== -1) {
      if (typeof one !== 'number') break;
      out.push(one & 255);
    }
    return out;
  }
  return [];
}
  function size(n) {
    n = Number(n);
    if (!Number.isInteger(n) || n < 0 || n > 16777216) throw new RangeError('Invalid array length');
    return n;
  }

  // Keep constructors stable across rule evaluations: jsLib may retain aliases
  // and use instanceof against them, while the native host functions refresh.
  var types = g.__readerJavaTypes;
  // 必须每次求值都跑: 类型表跨次复用(构造器要稳定), 但本轮新增的命名空间
  // (`java.nio` / `java.net` 等)只有执行到这里才能补进旧表。
  {
    types = types || {};
    // `new BigInteger(numBits, SecureRandom)`: Java 的随机大数构造(书源常用它
    // 生成 DH 私钥)。radix 位置拿到的是 SecureRandom 实例。
    function randomBigInt(bits) {
      bits = Number(bits) || 0;
      if (bits < 0) throw new Error('Invalid bit length');
      if (bits === 0) return 0n;
      var data = native('random', { length: Math.ceil(bits / 8) }), number = 0n;
      for (var i = 0; i < data.length; i++) number = (number << 8n) | BigInt(data[i] & 255);
      var excess = BigInt(data.length * 8 - bits);
      return excess > 0n ? number >> excess : number;
    }
    function big(value, radix) {
      if (value instanceof BigInteger) return value.value;
      if (Array.isArray(value) || ArrayBuffer.isView(value)) {
        var data = array(value), number = 0n;
        for (var i = 0; i < data.length; i++) number = (number << 8n) | BigInt(data[i] & 255);
        return data.length && data[0] < 0 ? number - (1n << BigInt(data.length * 8)) : number;
      }
      if (radix === undefined || Number(radix) === 10) return BigInt(value);
      var base = Number(radix), text = String(value).toLowerCase(), sign = 1n, number = 0n;
      if (base < 2 || base > 36) throw new RangeError('Invalid radix');
      if (text[0] === '-' || text[0] === '+') { sign = text[0] === '-' ? -1n : 1n; text = text.slice(1); }
      if (!text.length) throw new Error('Invalid integer');
      for (var i = 0; i < text.length; i++) {
        var digit = '0123456789abcdefghijklmnopqrstuvwxyz'.indexOf(text[i]);
        if (digit < 0 || digit >= base) throw new Error('Invalid integer');
        number = number * BigInt(base) + BigInt(digit);
      }
      return sign * number;
    }
    function BigInteger(value, radix) {
      if (!(this instanceof BigInteger)) return new BigInteger(value, radix);
      if (typeof value === 'number' && (Array.isArray(radix) || ArrayBuffer.isView(radix))) {
        var data = array(radix), number = 0n;
        for (var i = 0; i < data.length; i++) number = (number << 8n) | BigInt(data[i] & 255);
        this.value = BigInt(value) * number;
      } else if (radix instanceof SecureRandom) {
        this.value = randomBigInt(value);
      } else this.value = big(value, radix);
    }
    BigInteger.prototype.toString = function (radix) { return this.value.toString(radix === undefined ? 10 : Number(radix)); };
    BigInteger.prototype.toJSON = function () { return this.toString(); };
    BigInteger.prototype.intValue = function () { return Number(BigInt.asIntN(32, this.value)); };
    BigInteger.prototype.longValue = function () { return Number(BigInt.asIntN(64, this.value)); };
    BigInteger.prototype.byteValue = function () { return Number(BigInt.asIntN(8, this.value)); };
    BigInteger.prototype.signum = function () { return this.value < 0n ? -1 : this.value > 0n ? 1 : 0; };
    BigInteger.prototype.compareTo = function (v) { var other = big(v); return this.value < other ? -1 : this.value > other ? 1 : 0; };
    BigInteger.prototype.equals = function (v) { return v instanceof BigInteger && this.value === v.value; };
    BigInteger.prototype.add = function (v) { return new BigInteger(this.value + big(v)); };
    BigInteger.prototype.subtract = function (v) { return new BigInteger(this.value - big(v)); };
    BigInteger.prototype.multiply = function (v) { return new BigInteger(this.value * big(v)); };
    BigInteger.prototype.divide = function (v) { return new BigInteger(this.value / big(v)); };
    BigInteger.prototype.remainder = function (v) { return new BigInteger(this.value % big(v)); };
    BigInteger.prototype.mod = function (v) { var modulus = big(v); if (modulus <= 0n) throw new Error('Nonpositive modulus'); return new BigInteger((this.value % modulus + modulus) % modulus); };
    BigInteger.prototype.and = function (v) { return new BigInteger(this.value & big(v)); };
    BigInteger.prototype.or = function (v) { return new BigInteger(this.value | big(v)); };
    BigInteger.prototype.xor = function (v) { return new BigInteger(this.value ^ big(v)); };
    BigInteger.prototype.not = function () { return new BigInteger(~this.value); };
    BigInteger.prototype.shiftLeft = function (n) { return new BigInteger(this.value << BigInt(n)); };
    BigInteger.prototype.shiftRight = function (n) { return new BigInteger(this.value >> BigInt(n)); };
    BigInteger.prototype.negate = function () { return new BigInteger(-this.value); };
    BigInteger.prototype.abs = function () { return new BigInteger(this.value < 0n ? -this.value : this.value); };
    BigInteger.prototype.pow = function (n) { return new BigInteger(this.value ** BigInt(n)); };
    // `modPow`: 源脚本的 DH/签名计算。
    BigInteger.prototype.modPow = function (exponent, modulus) {
      var e = big(exponent), m = big(modulus);
      if (m <= 0n) throw new Error('Nonpositive modulus');
      if (e < 0n) throw new Error('Unsupported negative exponent');
      var result = 1n, base = ((this.value % m) + m) % m;
      while (e > 0n) {
        if (e & 1n) result = (result * base) % m;
        base = (base * base) % m;
        e >>= 1n;
      }
      return new BigInteger(result);
    };
    BigInteger.prototype.bitLength = function () { var n = this.value < 0n ? ~this.value : this.value; return n === 0n ? 0 : n.toString(2).length; };
    BigInteger.prototype.testBit = function (n) { return !!(this.value & (1n << BigInt(n))); };
    BigInteger.prototype.toByteArray = function () {
      var count = Math.max(1, Math.ceil((this.bitLength() + 1) / 8)), value = BigInt.asUintN(count * 8, this.value), out = new Array(count);
      for (var i = count - 1; i >= 0; i--) { out[i] = Number(BigInt.asIntN(8, value)); value >>= 8n; }
      return out;
    };
    BigInteger.valueOf = function (v) { return new BigInteger(v); };
    BigInteger.ZERO = new BigInteger(0); BigInteger.ONE = new BigInteger(1); BigInteger.TEN = new BigInteger(10);

    // Charset 实例/名字统一成宿主能识别的标签字符串。
    function charsetLabel(charset) {
      if (charset === undefined || charset === null) return 'UTF-8';
      if (charset instanceof Charset) return charset.name();
      if (typeof charset === 'object' && charset.__name) return String(charset.__name);
      return String(charset);
    }
    // `ISO-8859-1` 在 web 编码表里被映到 windows-1252, 0x80..0x9F 段与真实字节
    // 不一致。源脚本用这个字符集在**字节**与字符串之间做无损转换, 因此单独走
    // 纯 JS 的逐字节映射, 不经过宿主的编码表。
    function isLatin1(label) {
      return /^(iso[-_ ]?8859[-_ ]?1|latin[-_ ]?1)$/i.test(String(label).trim());
    }
    function latin1Decode(bytes) {
      var out = '';
      for (var i = 0; i < bytes.length; i++) out += String.fromCharCode(bytes[i] & 255);
      return out;
    }
    function latin1Encode(text) {
      var bytes = [];
      for (var i = 0; i < text.length; i++) {
        var code = text.charCodeAt(i);
        bytes.push(code <= 255 ? code : 63);
      }
      return bytes;
    }
    // Rhino 里 `new java.lang.String(x)` 是 Java 对象: `length()` / `getBytes()`
    // 等方法都要能用; 而 reader 侧历史实现同时把它当 JS 字符串用(`s.length` 取
    // 数字、模板拼接直接出文本)。这里用**普通对象 + 原型方法**承载: `toString` /
    // `valueOf` / `toJSON` 让它在字符串场合与之前一致, 同时实例上的 `length` 是
    // 可调用的函数(其 valueOf 给出长度, 所以 `s.length + 1` 这类旧写法仍然工作)。
    function JavaString(value, charset, length, encoding) {
      var text;
      if (Array.isArray(value) || ArrayBuffer.isView(value)) {
        var bytes = array(value);
        if (typeof charset === 'number') { bytes = bytes.slice(charset, charset + length); charset = encoding; }
        var label = charsetLabel(charset);
        text = isLatin1(label) ? latin1Decode(bytes) : native('decode', {data: bytes, charset: label});
      } else text = value === undefined || value === null ? '' : String(value);
      var result = {};
      Object.defineProperty(result, '__text', {value: text, enumerable: false, writable: true, configurable: true});
      Object.setPrototypeOf(result, JavaString.prototype);
      var lengthFn = function () { return String(result.__text).length; };
      lengthFn.valueOf = function () { return String(result.__text).length; };
      lengthFn.toString = function () { return String(String(result.__text).length); };
      Object.defineProperty(result, 'length', {value: lengthFn, enumerable: false, writable: false, configurable: true});
      return result;
    }
    JavaString.prototype = Object.create(Object.prototype);
    JavaString.prototype.constructor = JavaString;
    JavaString.prototype.toString = function () { return String(this.__text); };
    // 宿主在规则求值末尾要能把 Java String 还原成**裸文本**(不加 JSON 引号),
    // 旧实现靠 `instanceof String` 判定; 普通对象承载后改用这个显式标记。
    JavaString.prototype.__javaStringText = function () { return String(this.__text); };
    JavaString.prototype.valueOf = function () { return String(this.__text); };
    JavaString.prototype.toJSON = function () { return String(this.__text); };
    // `StringBuilder.toString()` 在 legado/Rhino 里仍是 Java String，源会继续
    // 调用 `getBytes(StandardCharsets.ISO_8859_1)`。该返回值在本兼容层由
    // prelude 的原生字符串承载，因此也必须具备 Java String 的方法面。
    // 原生字符串是 StringBuilder.toString() 的返回载体。方法不可枚举，避免
    // 书源对字符串做 for...in 时看见这个兼容属性。
    try {
      Object.defineProperty(String.prototype, 'getBytes', {
        value: function (charset) {
          var label = charsetLabel(charset);
          return isLatin1(label) ? latin1Encode(String(this)) : native('encode', {text: String(this), charset: label});
        },
        enumerable: false,
        writable: true,
        configurable: true
      });
    } catch (_e) {}
    JavaString.prototype.getBytes = function (charset) {
      var label = charsetLabel(charset);
      return isLatin1(label) ? latin1Encode(String(this)) : native('encode', {text: String(this), charset: label});
    };
    JavaString.prototype.equals = function (v) { return String(this) === String(v); };
    // Java String 的方法面: 源脚本按 Java 风格调用(`java.md5Encode(x).getBytes()`、
    // `java.digestHex(x).length()`、`k.substring(i, i3)`)。注意 `length` 在 Java 里
    // 是**方法**而 JS 字符串的 `length` 是属性 —— 包装对象上必须同时满足两种写法,
    // 否则书源里的 `k.length()` 会抛 "not a function"。
    JavaString.prototype.length = function () { return String(this).length; };
    JavaString.prototype.isEmpty = function () { return String(this).length === 0; };
    JavaString.prototype.charAt = function (i) { return String(this).charAt(size(i)); };
    JavaString.prototype.indexOf = function (needle, from) {
      return String(this).indexOf(String(needle), from === undefined ? 0 : size(from));
    };
    JavaString.prototype.lastIndexOf = function (needle) { return String(this).lastIndexOf(String(needle)); };
    JavaString.prototype.substring = function (start, end) {
      var text = String(this);
      start = size(start);
      return text.substring(start, end === undefined ? text.length : size(end));
    };
    JavaString.prototype.replace = function (from, to) {
      // Java 的 replace 是字面量替换；旧实现返回原生字符串，书源可能写正则，两者都支持。
      if (from instanceof RegExp) return String(this).replace(from, to);
      return String(this).split(String(from)).join(String(to));
    };
    JavaString.prototype.replaceAll = JavaString.prototype.replace;
    JavaString.prototype.split = function (sep) {
      if (sep instanceof RegExp) return String(this).split(sep);
      return String(this).split(String(sep));
    };
    JavaString.prototype.trim = function () { return String(this).trim(); };
    JavaString.prototype.toUpperCase = function () { return String(this).toUpperCase(); };
    JavaString.prototype.toLowerCase = function () { return String(this).toLowerCase(); };
    JavaString.prototype.contains = function (v) { return String(this).indexOf(String(v)) >= 0; };
    JavaString.prototype.startsWith = function (v) { return String(this).indexOf(String(v)) === 0; };
    JavaString.prototype.endsWith = function (v) {
      var text = String(this), tail = String(v);
      return text.length >= tail.length && text.slice(text.length - tail.length) === tail;
    };
    JavaString.prototype.toCharArray = function () { return String(this).split(''); };
    JavaString.prototype.compareTo = function (v) {
      var a = String(this), b = String(v);
      return a < b ? -1 : a > b ? 1 : 0;
    };
    // 旧实现里 java.* 字符串返回值是原生 JS 字符串, 书源会直接用 `slice` /
    // `match` / `indexOf` 等 String 方法。包装成对象后必须补齐这些方法,
    // 否则老书源会报 "xxx is not a function"。这里只挂载尚未定义的成员,
    // 已定义的 Java 方法(如 substring/replace)保持 Java 语义优先。
    // 字符串的 Symbol.iterator 不是字符串键, getOwnPropertyNames 带不过来;
    // 老书源里有 `[...hash]` / `for (ch of str)` 这种逐字符遍历写法。
    JavaString.prototype[Symbol.iterator] = String.prototype[Symbol.iterator];
    Object.getOwnPropertyNames(String.prototype).forEach(function (name) {
      if (name === 'constructor' || name === 'length') return;
      if (JavaString.prototype[name] !== undefined) return;
      var fn = String.prototype[name];
      if (typeof fn !== 'function') return;
      JavaString.prototype[name] = function () {
        return fn.apply(String(this), arguments);
      };
    });

    var Arrays = {
      copyOf: function (value, length) { return Arrays.copyOfRange(value, 0, size(length)); },
      copyOfRange: function (value, start, end) {
        start = size(start); end = size(end);
        if (start > value.length || start > end) throw new RangeError('Invalid array range');
        var out = array(value).slice(start, end);
        while (out.length < end - start) out.push(0);
        return out;
      }
    };
    function arraycopy(src, start, dest, offset, length) {
      start = size(start); offset = size(offset); length = size(length);
      if (start + length > src.length || offset + length > dest.length) throw new RangeError('Invalid copy range');
      var copy = array(src).slice(start, start + length);
      for (var i = 0; i < length; i++) dest[offset + i] = copy[i];
    }
    var Base64 = {
      getEncoder: function () { return {encodeToString: function (v) { return native('base64Encode', {data:array(v)}); }}; },
      getDecoder: function () { return {decode: function (v) { return native('base64Decode', {text:String(v)}); }}; }
    };
    // `Packages.android.util.Base64.decode(text, flags)`：Android 专有类，书源用它
    // 解 URL-safe base64（`-`/`_` 代替 `+`/`/`）。第二个参数是 flags 位掩码：
    //   URL_SAFE=8 / NO_PADDING=1 / NO_WRAP=2 / DEFAULT=0
    // 手机端（Android 运行时）本来就有这个类，桌面端必须补齐，否则脚本里直接
    // `cannot read property 'util' of undefined` —— 这类源表现为解码出 null、
    // 发现页/正文空白。
    //
    // 实现上先按 flags 还原字符表与补位，再复用同一套宽松解码；`NO_PADDING`
    // 与 `URL_SAFE` 是实际会遇到的两种，其余位按 DEFAULT 处理即可。
    var AndroidBase64 = {
      DEFAULT: 0,
      NO_PADDING: 1,
      NO_WRAP: 2,
      CRLF: 4,
      URL_SAFE: 8,
      NO_CLOSE: 16,
      decode: function (text, flags) {
        var raw = String(text == null ? '' : text);
        var bits = Number(flags) || 0;
        if (bits & 8) raw = raw.replace(/-/g, '+').replace(/_/g, '/');
        // NO_WRAP/CRLF 只影响编码侧换行，解码时把空白去掉即可。
        raw = raw.replace(/\s+/g, '');
        if (!(bits & 1)) { while (raw.length % 4) raw += '='; }
        return native('base64Decode', { text: raw });
      },
      encodeToString: function (data, flags) {
        var encoded = String(native('base64Encode', { data: array(data) }));
        var bits = Number(flags) || 0;
        if (bits & 8) encoded = encoded.replace(/\+/g, '-').replace(/\//g, '_');
        if (bits & 1) encoded = encoded.replace(/=+$/, '');
        if (bits & 2) encoded = encoded.replace(/(.{76})/g, '$1\n');
        return encoded;
      }
    };
    // `Packages.java.util.UUID.randomUUID()`: 源脚本用它生成设备指纹字段,
    // 与 legado 的 java.randomUUID() 同源。
    var UUID = {
      randomUUID: function () { return String(g.java.uuid()); },
      fromString: function (text) { return String(text); },
      toString: function () { return String(g.java.uuid()); }
    };
    // `Packages.java.nio.charset.Charset.forName('ISO-8859-1')`: 源脚本用它把
    // 二进制字节与 JS 字符串互转。只需承载名字供
    // JavaString 构造器识别，不需要真正的编码表。
    var charsetInstances = {};
    // Rhino 里 `Packages.java.nio.charset.Charset` 就是类本身, 静态方法挂在类上。
    function Charset(name) { this.__name = String(name || 'UTF-8'); }
    Charset.prototype.name = function () { return this.__name; };
    Charset.prototype.toString = function () { return this.__name; };
    Charset.forName = function (name) {
      var key = String(name).toUpperCase();
      if (!charsetInstances[key]) charsetInstances[key] = new Charset(key);
      return charsetInstances[key];
    };
    Charset.isSupported = function () { return true; };
    Charset.defaultCharset = function () { return Charset.forName('UTF-8'); };
    // `Packages.java.nio.charset.StandardCharsets.ISO_8859_1` 是部分 APP 源
    // 生成 MD5 密钥时的实际写法。`Charset.forName` 单独存在不等于该常量存在，
    // 缺了它会在正文脚本里直接报 `cannot read property 'ISO_8859_1' of undefined`。
    var StandardCharsets = {
      ISO_8859_1: Charset.forName('ISO-8859-1'),
      US_ASCII: Charset.forName('US-ASCII'),
      UTF_8: Charset.forName('UTF-8'),
      UTF_16: Charset.forName('UTF-16'),
      UTF_16BE: Charset.forName('UTF-16BE'),
      UTF_16LE: Charset.forName('UTF-16LE')
    };
    // `Packages.java.util.Collections`: 源脚本(如聚合类源的 header 脚本)用
    // `Collections.reverse(list)` 就地反转载荷列表, 缺了它会抛
    // "cannot read property 'reverse' of undefined" 并中断整段脚本。
    //
    // 只交换**索引位置**, 绝不用 `array()` 重建列表: 入参常是 jsoup 包装过的
    // Elements(元素带 `attr()`/`text()` 方法), 经 `array()` 转换会退化成纯数据,
    // 之后 `char.attr('href')` 就成了 "not a function"。
    // 与 Java 一致地返回 undefined 并修改入参。
    function swapInPlace(list) {
      var len = list.length >>> 0;
      for (var i = 0, j = len - 1; i < j; i++, j--) {
        var tmp = list[i]; list[i] = list[j]; list[j] = tmp;
      }
      return len;
    }
    var Collections = {
      reverse: function (list) {
        if (list === null || list === undefined) throw new TypeError('Collections.reverse: list is null');
        swapInPlace(list);
        return undefined;
      },
      sort: function (list) {
        if (list === null || list === undefined) throw new TypeError('Collections.sort: list is null');
        // 只对可比较的标量列表排序; 同样按索引写回, 保留元素对象本身。
        var len = list.length >>> 0;
        var order = [];
        for (var i = 0; i < len; i++) order.push(list[i]);
        order.sort(function (a, b) { return a < b ? -1 : a > b ? 1 : 0; });
        for (var k = 0; k < len; k++) list[k] = order[k];
        return undefined;
      },
      shuffle: function (list) {
        if (list === null || list === undefined) throw new TypeError('Collections.shuffle: list is null');
        var len = list.length >>> 0;
        for (var i = len - 1; i > 0; i--) {
          var j = Math.floor(Math.random() * (i + 1));
          var tmp = list[i]; list[i] = list[j]; list[j] = tmp;
        }
        return undefined;
      },
      emptyList: function () { return []; },
      singletonList: function (value) { return [value]; }
    };
    function SecureRandom() {}
    SecureRandom.prototype.nextBytes = function (target) { arraycopy(native('random', {length:target.length}), 0, target, 0, target.length); };
    SecureRandom.prototype.generateSeed = function (length) { return native('random', {length:size(length)}); };
    /// `nextInt(bound)`：Java 语义是 `[0, bound)` 的均匀整数；无参时返回任意 32 位有符号数。
    /// 书源拿它生成 IV / nonce（`SecureRandom().nextInt(256)`），没有它整段加密初始化
    /// 就抛 `not a function`。
    SecureRandom.prototype.nextInt = function (bound) {
      var bytes = native('random', {length:4});
      var value = ((bytes[0] & 255) << 24) | ((bytes[1] & 255) << 16)
                | ((bytes[2] & 255) << 8) | (bytes[3] & 255);
      if (bound === undefined) return value | 0;
      var n = Math.floor(Number(bound));
      if (!(n > 0)) throw new Error('bound must be positive');
      // 取模会有极轻微偏差，但对 IV 用途（256/0x7fffffff）完全够用。
      return ((value >>> 0) % n);
    };
    /// `nextLong()` 的近似实现。
    SecureRandom.prototype.nextLong = function () {
      var bytes = native('random', {length:8}), v = 0n;
      for (var i = 0; i < 8; i++) v = (v << 8n) | BigInt(bytes[i] & 255);
      return Number(BigInt.asIntN(64, v));
    };
    var MessageDigest = {getInstance: function (algorithm) {
      var pending = [];
      return {
        update: function (value) { pending = pending.concat(typeof value === 'number' ? [value] : array(value)); },
        digest: function (value) { var data = pending.concat(value === undefined ? [] : array(value)); pending = []; return native('digest', {algorithm:String(algorithm), data:data}); },
        reset: function () { pending = []; }
      };
    }};
    // Rhino 里把 Java 类当普通函数调用等价于 `new`(NativeJavaClass.call 会走
    // construct), 书源大量写 `key = SecretKeySpec(ks, "AES")` 而不带 new。
    function SecretKeySpec(value, algorithm) {
      if (!(this instanceof SecretKeySpec)) return new SecretKeySpec(value, algorithm);
      this.bytes = array(value); this.algorithm = String(algorithm);
    }
    SecretKeySpec.prototype.getEncoded = function () { return this.bytes.slice(); };
    function IvParameterSpec(value) {
      if (!(this instanceof IvParameterSpec)) return new IvParameterSpec(value);
      this.bytes = array(value);
    }
    /// `javax.crypto.spec.GCMParameterSpec(tagLen, iv)`：AES/GCM 的 IV 载体。
    /// 与 `IvParameterSpec` 的区别是多了认证标签长度（128 = 16 字节），
    /// 而 `Cipher` 侧只关心 `bytes` —— 两者可互换交给 `init`。
    /// 缺了它，源里 `new Packages.javax.crypto.spec.GCMParameterSpec(128, iv)`
    /// 会抛 `not a function`，整段解密（进而整个搜索）失败。
    function GCMParameterSpec(tagLen, value) {
      if (!(this instanceof GCMParameterSpec)) return new GCMParameterSpec(tagLen, value);
      // 兼容 `(iv)` 单参形式：此时第一个参数其实是 IV。
      if (value === undefined) { value = tagLen; tagLen = 128; }
      this.bytes = array(value);
      this.tLen = Number(tagLen) || 128;
    }
    /// `javax.crypto.Mac`：书源写
    /// `Packages.javax.crypto.Mac.getInstance("HmacSHA256")`
    /// `.init(new SecretKeySpec(k, alg))` / `.doFinal(bytes)` 做签名。
    /// 缺了它就是 `cannot read property 'getInstance' of undefined`，
    /// 整个搜索地址拼不出来 —— 表现为「搜索 0 条」。
    var Mac = {getInstance: function (algorithm) {
      var key = null, pending = [];
      return {
        init: function (spec) { key = spec && spec.bytes ? spec.bytes : array(spec); },
        update: function (value) { pending = pending.concat(typeof value === 'number' ? [value] : array(value)); },
        // `doFinal()` 无参时用 update 累积的数据；带参时把参数接在后面。
        doFinal: function (value) {
          if (!key) throw new Error('Mac not initialized');
          var data = pending.concat(value === undefined ? [] : array(value));
          pending = [];
          return native('mac', {algorithm:String(algorithm), key:key, data:data});
        },
        reset: function () { pending = []; }
      };
    }};
    var Cipher = {ENCRYPT_MODE:1, DECRYPT_MODE:2, getInstance:function (transformation) {
      var key, iv = [], encrypt, pending = [];
      return {
        init: function (mode, k, v) { if (mode !== 1 && mode !== 2) throw new Error('Unsupported cipher mode'); encrypt = mode === 1; key = k.bytes; iv = v ? v.bytes : []; pending = []; },
        update: function (data) { pending = pending.concat(array(data)); return []; },
        doFinal: function (data) { if (!key) throw new Error('Cipher not initialized'); var input = pending.concat(data === undefined ? [] : array(data)); pending = []; return native('cipher', {transformation:String(transformation), encrypt:encrypt, key:key, iv:iv, data:input}); }
      };
    }};
    /// `java.nio.ByteBuffer`（只实现书源真正用到的那一小块）：
  /// `allocate(n)` / `putInt` / `put` / `putShort` / `array()` / `flip()`。
  ///
  /// 来源写法是 `ByteBuffer.allocate(4).putInt(n|0).array()` 取 4 字节**大端**
  /// 整数（pow 校验里拼 counter）。缺它整个搜索前置校验就抛
  /// `not a function`，界面只显示「搜索 0 条」。
  function ByteBuffer(capacity) {
    this.buffer = new Array(capacity).fill(0);
    this.position = 0;
    this.limit = capacity;
  }
  ByteBuffer.allocate = function (capacity) { return new ByteBuffer(Number(capacity) || 0); };
  ByteBuffer.prototype._ensure = function (need) {
    if (this.position + need <= this.buffer.length) return;
    while (this.buffer.length < this.position + need) this.buffer.push(0);
  };
  ByteBuffer.prototype.put = function (value) {
    if (typeof value === 'number') {
      this._ensure(1);
      this.buffer[this.position++] = value & 255;
    } else {
      var bytes = array(value);
      this._ensure(bytes.length);
      for (var i = 0; i < bytes.length; i++) this.buffer[this.position++] = bytes[i] & 255;
    }
    return this;
  };
  ByteBuffer.prototype.putInt = function (value) {
    var v = Number(value) >>> 0;
    this._ensure(4);
    // Java 的 ByteBuffer 默认大端
    this.buffer[this.position++] = (v >>> 24) & 255;
    this.buffer[this.position++] = (v >>> 16) & 255;
    this.buffer[this.position++] = (v >>> 8) & 255;
    this.buffer[this.position++] = v & 255;
    return this;
  };
  ByteBuffer.prototype.putShort = function (value) {
    var v = Number(value) & 0xFFFF;
    this._ensure(2);
    this.buffer[this.position++] = (v >>> 8) & 255;
    this.buffer[this.position++] = v & 255;
    return this;
  };
  ByteBuffer.prototype.flip = function () { this.limit = this.position; this.position = 0; return this; };
  ByteBuffer.prototype.rewind = function () { this.position = 0; return this; };
  ByteBuffer.prototype.remaining = function () { return this.limit - this.position; };
  ByteBuffer.prototype.array = function () { return this.buffer.slice(0, this.limit); };
  ByteBuffer.prototype.get = function (index) {
    return this.buffer[index === undefined ? this.position++ : index] & 255;
  };

  function ByteArrayInputStream(data) { this.bytes = array(data); this.position = 0; }
    ByteArrayInputStream.prototype.read = function (target, offset, length) {
      if (this.position >= this.bytes.length) return -1;
      if (target === undefined) return this.bytes[this.position++] & 255;
      offset = offset === undefined ? 0 : size(offset);
      length = length === undefined ? target.length - offset : size(length);
      if (offset + length > target.length) throw new RangeError('Invalid read range');
      var count = Math.min(length, this.bytes.length - this.position);
      arraycopy(this.bytes, this.position, target, offset, count); this.position += count; return count;
    };
    ByteArrayInputStream.prototype.close = function () {};
    function ByteArrayOutputStream() { this.bytes = []; }
    ByteArrayOutputStream.prototype.write = function (data, offset, length) {
      if (typeof data === 'number') this.bytes.push((data << 24) >> 24);
      else { offset = offset || 0; var values = array(data); this.bytes = this.bytes.concat(values.slice(offset, length === undefined ? undefined : offset + length)); }
    };
    ByteArrayOutputStream.prototype.toByteArray = function () { return this.bytes.slice(); };
    ByteArrayOutputStream.prototype.close = function () {};
    // `new Packages.java.util.zip.InflaterInputStream(stream)`: 书源把已经有完整
    // zlib 头尾的数据逐块解压。源脚本会自己做 XOR, 这里不能再次异或。
    function InflaterInputStream(input, inflater) {
      var bytes = input && input.bytes ? input.bytes.slice(input.position) : array(input);
      this.bytes = array(native('inflate', {
        data: bytes,
        nowrap: !!(inflater && inflater.nowrap)
      }));
      this.position = 0;
      var stream = this;
      this.read = function (target, offset, length) {
        if (stream.position >= stream.bytes.length) return -1;
        if (target === undefined) return stream.bytes[stream.position++] & 255;
        offset = offset === undefined ? 0 : size(offset);
        length = length === undefined ? target.length - offset : size(length);
        if (offset + length > target.length) throw new RangeError('Invalid read range');
        var count = Math.min(length, stream.bytes.length - stream.position);
        arraycopy(stream.bytes, stream.position, target, offset, count);
        stream.position += count;
        return count;
      };
      this.close = function () {};
      this.available = function () { return stream.bytes.length - stream.position; };
      this.skip = function (count) {
        var consumed = Math.min(size(count), stream.available());
        stream.position += consumed;
        return consumed;
      };
    }
    // `Packages.java.util.zip.Inflater(nowrap)`：书源把压缩段（raw deflate）逐块
    // 解压成 JSON。Java 的用法是一组固定调用序列：
    //
    //   var inf = new Inflater(true);
    //   inf.setInput(raw, 1, raw.length - 1);
    //   while (!inf.finished()) { n = inf.inflate(buf); if (n <= 0) break; out.write(buf, 0, n); }
    //   inf.end();
    //
    // 桌面端没有流式解压，这里在 `setInput` 时一次性解完、缓存结果，`inflate(buf)`
    // 只负责按 buf 长度分块搬运。这样上面那套循环不需要客户端改动。
    //
    // **nowrap 必须区分**：`Inflater(true)` 对应 raw deflate（不带 zlib 头），
    // 用 zlib 解码会报 "invalid zlib header"。书源用 `(true)`，所以默认按 raw。
    function Inflater(nowrap) {
      this.nowrap = nowrap !== false;
      this.input = [];
      this.output = [];
      this.position = 0;
      this.finishedFlag = false;
    }
    Inflater.prototype.setInput = function (data, offset, length) {
      offset = offset === undefined ? 0 : size(offset);
      var values = array(data);
      length = length === undefined ? values.length - offset : size(length);
      this.input = values.slice(offset, offset + length);
      try {
        this.output = array(native('inflate', { data: this.input, nowrap: this.nowrap }));
      } catch (e) {
        this.output = [];
      }
      this.position = 0;
      this.finishedFlag = false;
      return this.input.length;
    };
    Inflater.prototype.inflate = function (target) {
      if (this.position >= this.output.length) { this.finishedFlag = true; return 0; }
      var remaining = this.output.length - this.position;
      if (!target) { this.position = this.output.length; this.finishedFlag = true; return remaining; }
      var capacity = target.length === undefined ? 0 : target.length;
      var count = Math.min(capacity || remaining, remaining);
      for (var i = 0; i < count; i++) target[i] = this.output[this.position + i];
      this.position += count;
      this.finishedFlag = this.position >= this.output.length;
      return count;
    };
    Inflater.prototype.finished = function () { return this.finishedFlag || this.position >= this.output.length; };
    Inflater.prototype.needsInput = function () { return this.position >= this.output.length; };
    Inflater.prototype.getRemaining = function () { return Math.max(0, this.output.length - this.position); };
    Inflater.prototype.reset = function () { this.position = 0; this.finishedFlag = false; };
    Inflater.prototype.end = function () { this.input = []; this.output = []; this.position = 0; this.finishedFlag = true; };
    function GZIPInputStream(input) { ByteArrayInputStream.call(this, native('gunzip', {data:input.bytes.slice(input.position)})); }
    GZIPInputStream.prototype = Object.create(ByteArrayInputStream.prototype);
    function GZIPOutputStream(output) {
      this.out = output;
      this.pending = [];
      this.closed = false;
    }
    GZIPOutputStream.prototype.write = function (data, offset, length) {
      if (typeof data === 'number') this.pending.push(data & 255);
      else {
        offset = offset || 0;
        var values = array(data);
        this.pending = this.pending.concat(values.slice(offset, length === undefined ? undefined : offset + length));
      }
    };
    GZIPOutputStream.prototype.close = function () {
      if (this.closed) return;
      this.closed = true;
      // Java 的 GZIPOutputStream 在 close 时写出完整 gzip 流; 目标流按 Java 语义
      // 接收 int 数组, 因此这里保持数组写入(不用 latin1 字符串)。
      this.out.write(native('gzip', { data: this.pending }));
      this.pending = [];
    };
    GZIPOutputStream.prototype.finish = GZIPOutputStream.prototype.close;
    // `Packages.java.io.BufferedReader(new InputStreamReader(stream,'UTF-8'))`:
    // 源脚本用它逐行读取响应文本。这里按 ByteArrayInputStream 的字节缓冲读出
    // 全部内容再切行, readLine 逐行返回。
    function InputStreamReader(stream, charset) {
      this.stream = stream;
      this.charset = charset || 'UTF-8';
      this.text = null;
      this.lines = null;
      this.index = 0;
    }
    InputStreamReader.prototype.__text = function () {
      if (this.text !== null) return this.text;
      var bytes = [];
      if (this.stream instanceof ByteArrayInputStream) bytes = this.stream.bytes.slice(this.stream.position);
      else if (this.stream && typeof this.stream.read === 'function') {
        var chunk = [0];
        while (this.stream.read(chunk, 0, 1) > 0) bytes.push(chunk[0] & 255);
      } else bytes = array(this.stream);
      this.text = native('decode', { data: bytes, charset: this.charset });
      return this.text;
    };
    InputStreamReader.prototype.readLine = function () {
      if (this.lines === null) this.lines = String(this.__text()).split(/\r\n|\r|\n/);
      return this.index < this.lines.length ? this.lines[this.index++] : null;
    };
    InputStreamReader.prototype.read = function () {
      var text = this.__text();
      if (this.index >= text.length) return -1;
      return text.charCodeAt(this.index++);
    };
    InputStreamReader.prototype.close = function () { if (this.stream && this.stream.close) this.stream.close(); };
    function BufferedReader(reader) {
      this.reader = reader instanceof InputStreamReader ? reader : new InputStreamReader(reader, 'UTF-8');
    }
    BufferedReader.prototype.readLine = function () { return this.reader.readLine(); };
    BufferedReader.prototype.read = function () { return this.reader.read(); };
    BufferedReader.prototype.close = function () { this.reader.close(); };

    // `Packages.java.net.URL(url).openConnection()`: 源脚本用它发送带自定义
    // 请求头的二进制 POST 并读回响应。底层复用宿主 `__http_bytes` 原语:
    // 请求体按 latin1 还原成原始字节, 响应体按 latin1 原样带回。
    //
    // 这里的 `this` 指向每次 openConnection 新建的连接对象, 与其他 Java 互操作
    // 类的"实例方法"写法一致。
    function URLImpl(url) { this.__url = String(url); }
    URLImpl.prototype.toString = function () { return this.__url; };
    URLImpl.prototype.getProtocol = function () { var m = /^([a-z][a-z0-9+.-]*):/i.exec(this.__url); return m ? m[1].toLowerCase() : ''; };
    URLImpl.prototype.getHost = function () { var m = /^[a-z][a-z0-9+.-]*:\/\/([^/:?#]+)/i.exec(this.__url); return m ? m[1] : ''; };
    URLImpl.prototype.getPath = function () { var m = /^[a-z][a-z0-9+.-]*:\/\/[^/]*(\/[^?#]*)?/i.exec(this.__url); return m && m[1] ? m[1] : ''; };
    URLImpl.prototype.openConnection = function () { return new UrlConnection(this.__url); };
    URLImpl.prototype.openStream = function () { return this.openConnection().getInputStream(); };

    function UrlConnection(url) {
      this.__url = String(url);
      this.__method = 'GET';
      this.__headers = {};
      this.__body = [];
      this.__response = null;
    }
    UrlConnection.prototype.setRequestMethod = function (method) { this.__method = String(method).toUpperCase(); };
    UrlConnection.prototype.setRequestProperty = function (key, value) { this.__headers[String(key)] = value == null ? '' : String(value); };
    UrlConnection.prototype.setDoOutput = function () { this.__doOutput = true; };
    UrlConnection.prototype.setDoInput = function () {};
    UrlConnection.prototype.setConnectTimeout = function () {};
    UrlConnection.prototype.setReadTimeout = function () {};
    UrlConnection.prototype.setUseCaches = function () {};
    UrlConnection.prototype.setInstanceFollowRedirects = function () {};
    UrlConnection.prototype.getOutputStream = function () {
      var connection = this;
      return {
        write: function (data, offset, length) {
          if (typeof data === 'number') connection.__body.push(data & 255);
          else {
            offset = offset || 0;
            var values = array(data);
            connection.__body = connection.__body.concat(
              values.slice(offset, length === undefined ? undefined : offset + length)
            );
          }
        },
        flush: function () {},
        close: function () {}
      };
    };
    UrlConnection.prototype.__send = function () {
      if (this.__response !== null) return this.__response;
      var body = this.__body.map(function (b) { return String.fromCharCode(b & 255); }).join('');
      this.__response = JSON.parse(g.__http_bytes(
        this.__method, this.__url, body, JSON.stringify(this.__headers)
      ));
      return this.__response;
    };
    UrlConnection.prototype.getResponseCode = function () { return Number(this.__send().code) || 0; };
    UrlConnection.prototype.getInputStream = function () {
      var response = this.__send();
      var bytes = String(response.body == null ? '' : response.body).split('').map(function (c) { return c.charCodeAt(0) & 255; });
      return new ByteArrayInputStream(bytes);
    };
    UrlConnection.prototype.getErrorStream = function () { return this.getInputStream(); };
    UrlConnection.prototype.getHeaderField = function (key) {
      var headers = this.__send().headers || {}, name = String(key).toLowerCase();
      for (var k in headers) {
        if (Object.prototype.hasOwnProperty.call(headers, k) && k.toLowerCase() === name) return headers[k];
      }
      return null;
    };
    UrlConnection.prototype.disconnect = function () {};

    // `Packages.okhttp3.OkHttpClient`: 部分 APP 源用 okhttp 的同步调用拿原始
    // 响应字节，再自行 XOR/Inflate。桌面端没有 JVM, 这里复用上面的
    // `UrlConnection` 字节路径; 网络请求仍经过书源 header、cookie、代理与限速器。
    function OkHttpClient() {}
    OkHttpClient.prototype.newCall = function (request) { return new OkHttpCall(request); };
    function OkHttpRequest(url, method, headers, body) {
      this.__url = String(url);
      this.__method = String(method || 'GET').toUpperCase();
      this.headers = headers || {};
      this.body = body === undefined || body === null ? '' : String(body);
    }
    function OkHttpRequestBuilder() {
      this.__url = '';
      this.__method = 'GET';
      this.headers = {};
      this.body = '';
    }
    // OkHttp 源只构造 `Request.Builder`, 不需要可调用的 `Request` 类本身。
    // 普通对象能确保嵌套的静态成员在 rquickjs 中按函数保存, 而不是被桥接成
    // 某个实例属性。
    function OkHttpRequestBuilderClass() {
      return new OkHttpRequestBuilder();
    }
    var OkHttpRequestType = {Builder: OkHttpRequestBuilderClass};
    OkHttpRequestBuilder.prototype.url = function (value) { this.__url = String(value); return this; };
    OkHttpRequestBuilder.prototype.addHeader = function (key, value) {
      this.headers[String(key)] = value == null ? '' : String(value);
      return this;
    };
    OkHttpRequestBuilder.prototype.header = function (key, value) {
      return this.addHeader(key, value);
    };
    OkHttpRequestBuilder.prototype.method = function (method, body) {
      this.__method = String(method).toUpperCase();
      this.body = body == null ? '' : String(body);
      return this;
    };
    OkHttpRequestBuilder.prototype.get = function () { return this.method('GET'); };
    OkHttpRequestBuilder.prototype.post = function (body) { return this.method('POST', body); };
    OkHttpRequestBuilder.prototype.build = function () {
      return new OkHttpRequest(this.__url, this.__method, this.headers, this.body);
    };
    function OkHttpResponseBody(bytes) {
      this.bytesData = bytes;
      this.consumed = false;
    }
    OkHttpResponseBody.prototype.bytes = function () { return this.bytesData.slice(); };
    OkHttpResponseBody.prototype.string = function () {
      try { return String(g.__bytes_to_str(JSON.stringify(this.bytesData), 'UTF-8')); } catch (_e) { return ''; }
    };
    OkHttpResponseBody.prototype.byteStream = function () { return new ByteArrayInputStream(this.bytes()); };
    OkHttpResponseBody.prototype.contentLength = function () { return this.bytesData.length; };
    OkHttpResponseBody.prototype.close = function () {};
    OkHttpResponseBody.prototype.source = function () {
      var body = this;
      return {readByteArray: function () { return body.bytes(); }};
    };
    function OkHttpResponse(url, code, headers, bytes) {
      this.__url = String(url);
      this.__code = Number(code) || 0;
      this.__headers = headers || {};
      this.__body = new OkHttpResponseBody(bytes);
    }
    OkHttpResponse.prototype.body = function () { return this.__body; };
    OkHttpResponse.prototype.code = function () { return this.__code; };
    OkHttpResponse.prototype.isSuccessful = function () { return this.__code >= 200 && this.__code < 300; };
    OkHttpResponse.prototype.url = function () { return new URLImpl(this.__url); };
    OkHttpResponse.prototype.header = function (key) {
      var name = String(key).toLowerCase();
      for (var k in this.__headers) {
        if (Object.prototype.hasOwnProperty.call(this.__headers, k) && k.toLowerCase() === name) return this.__headers[k];
      }
      return null;
    };
    OkHttpResponse.prototype.close = function () {};
    function OkHttpCall(request) { this.request = request; this.executed = false; }
    OkHttpCall.prototype.execute = function () {
      if (this.executed) throw new Error('Already Executed');
      this.executed = true;
      var request = this.request;
      if (!request || !request.__url) throw new Error('OkHttp Request.url 不能为空');
      if (request.__method === 'GET' || request.__method === 'HEAD') {
        request.body = '';
      }
      if (typeof g.__http_bytes !== 'function') throw new Error('OkHttp: HTTP 字节原语不可用');
      var response = JSON.parse(g.__http_bytes(
        request.__method, request.__url, request.body, JSON.stringify(request.headers)
      ));
      var bytes = String(response.body == null ? '' : response.body).split('').map(function (c) {
        return c.charCodeAt(0) & 255;
      });
      return new OkHttpResponse(response.url || request.__url, response.code, response.headers, bytes);
    };
    OkHttpCall.prototype.cancel = function () {};
    OkHttpCall.prototype.isCanceled = function () { return false; };
    OkHttpCall.prototype.isExecuted = function () { return this.executed; };

    // 每次求值都要重新跑一遍这个脚本(prelude 的 `java` 对象是本次新建的), 但
    // Rhino 风格的类型表要跨次复用: jsLib 可能持有 `Packages.java.math.BigInteger`
    // 这类别名, 重建构造器会让 `instanceof` 失效。因此只补新字段, 不整体替换。
    types = g.__readerJavaTypes || {};
    // `java.net.URLEncoder` / `URLDecoder`: 书源用它拼/解查询串。
    // `encode` 走 UTF-8 且把空格记为 `+`(与 Java 一致); `decode` 接受 `+` 当空格。
    var URLEncoderImpl = {
      encode: function (v, charset) {
        var enc = charset === undefined || charset === null ? 'UTF-8' : String(charset);
        if (/^utf-?8$/i.test(enc)) {
          return encodeURIComponent(String(v)).replace(/%20/g, '+');
        }
        try {
          var bytes = JSON.parse(String(g.__str_to_bytes(String(v), enc)));
          var hex = '0123456789ABCDEF', parts = [];
          for (var i = 0; i < bytes.length; i++) {
            var b = bytes[i] & 0xff;
            var plain = (b >= 0x30 && b <= 0x39) || (b >= 0x41 && b <= 0x5a) ||
              (b >= 0x61 && b <= 0x7a) || b === 0x2d || b === 0x2e || b === 0x5f || b === 0x2a;
            if (plain) parts.push(String.fromCharCode(b));
            else if (b === 0x20) parts.push('+');
            else parts.push('%' + hex.charAt(b >> 4) + hex.charAt(b & 15));
          }
          return parts.join('');
        } catch (e) {
          return encodeURIComponent(String(v)).replace(/%20/g, '+');
        }
      }
    };
    var URLDecoderImpl = {
      decode: function (v, charset) {
        var enc = charset === undefined || charset === null ? 'UTF-8' : String(charset);
        var text = String(v).replace(/\+/g, ' ');
        try {
          if (/^utf-?8$/i.test(enc)) {
            return decodeURIComponent(text);
          }
          var bytes = [];
          for (var i = 0; i < text.length; i++) {
            var ch = text.charAt(i);
            if (ch === '%' && i + 2 < text.length) {
              var code = parseInt(text.substr(i + 1, 2), 16);
              if (!isNaN(code)) { bytes.push(code & 0xff); i += 2; continue; }
            }
            bytes.push(ch.charCodeAt(0) & 0xff);
          }
          return String(g.__bytes_to_str(JSON.stringify(bytes), enc));
        } catch (e) { return text; }
      }
    };
    // `Packages.java.lang.reflect.Array.newInstance(SomeType, n)`：书源用它申请
    // 一个定长字节缓冲区交给 `Inflater.inflate(buf)`。桌面端返回普通数组即可。
    var ReflectArray = {
      newInstance: function (componentType, length) {
        var n = Math.max(0, Number(length) || 0);
        var out = new Array(n);
        for (var i = 0; i < n; i++) out[i] = 0;
        return out;
      }
    };
    var nextTypes = {
      math:{BigInteger:BigInteger},
      lang:{
        String:JavaString,
        System:{currentTimeMillis:function () { return Date.now(); }, arraycopy:arraycopy},
        Thread:{sleep:function (ms) { g.__sleep(Number(ms)); }},
        reflect:{Array:ReflectArray},
        // `Packages.java.lang.Byte.TYPE`：书源写
        // `Array.newInstance(Byte.TYPE, 8192)` 申请字节缓冲区。桌面端只需要
        // 一个非空占位（ReflectArray 会忽略元素类型），但不能缺 —— 缺了就是
        // `cannot read property 'TYPE' of undefined`，解压整段失败。
        Byte:{TYPE:'byte'}
      },
      util:{Arrays:Arrays, Base64:Base64, Collections:Collections, UUID:UUID, zip:{GZIPInputStream:GZIPInputStream, GZIPOutputStream:GZIPOutputStream, Inflater:Inflater, InflaterInputStream:InflaterInputStream}},
      security:{MessageDigest:MessageDigest, SecureRandom:SecureRandom},
      io:{ByteArrayInputStream:ByteArrayInputStream, ByteArrayOutputStream:ByteArrayOutputStream, InputStreamReader:InputStreamReader, BufferedReader:BufferedReader},
      net:{URL:URLImpl, HttpURLConnection:UrlConnection, URLEncoder:URLEncoderImpl, URLDecoder:URLDecoderImpl},
      nio:{charset:{Charset:Charset, StandardCharsets:StandardCharsets}, ByteBuffer:ByteBuffer},
      crypto:{Cipher:Cipher, Mac:Mac, spec:{SecretKeySpec:SecretKeySpec, IvParameterSpec:IvParameterSpec, GCMParameterSpec:GCMParameterSpec}},
      // `Packages.android.util.Base64` 等 Android 专有类。手机端天然存在，桌面端
      // 必须补；缺了就是 `cannot read property 'util' of undefined`。
      android:{util:{Base64:AndroidBase64}},
      okhttp3:{
        OkHttpClient:OkHttpClient,
        Request:OkHttpRequestType
      }
    };
    // 已有类型保持原引用(jsLib 里的 `instanceof` 依赖它), 新类型按首次注册。
    Object.keys(nextTypes).forEach(function (namespace) {
      var current = types[namespace];
      if (!current || typeof current !== 'object') { types[namespace] = nextTypes[namespace]; return; }
      Object.keys(nextTypes[namespace]).forEach(function (name) {
        if (current[name] === undefined) current[name] = nextTypes[namespace][name];
      });
      // `util.zip` / `nio.charset` / `crypto.spec` 这类二级命名空间同样按需补齐。
      Object.keys(nextTypes[namespace]).forEach(function (name) {
        var nextValue = nextTypes[namespace][name];
        if (nextValue && typeof nextValue === 'object' && current[name] && typeof current[name] === 'object' && !Array.isArray(nextValue)) {
          Object.keys(nextValue).forEach(function (inner) {
            if (current[name][inner] === undefined) current[name][inner] = nextValue[inner];
          });
        }
      });
    });
    g.__readerJavaTypes = types;
  }
  g.java.math = types.math;
  Object.assign(g.java.lang, types.lang);
  Object.assign(g.java.util, types.util);
  g.java.security = types.security;
  g.java.io = types.io;
  g.java.net = types.net;
  g.java.nio = types.nio;
  g.javax = {crypto:types.crypto};
  g.Packages.javax = g.javax;
  g.Packages.okhttp3 = types.okhttp3;
  // `Packages.android.util.Base64`：Android 专有命名空间，不属于 `Packages.java`，
  // 因此不能挂在 g.java 上 —— 书源写的就是 `Packages.android.util.Base64.decode(...)`。
  g.Packages.android = types.android;
})(globalThis);
