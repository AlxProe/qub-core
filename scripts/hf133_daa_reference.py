"""Independent, integer-only QUB ASERT reference for offline release checks.

The ASERT polynomial and truncation follow the BCHN public specification:
https://upgradespecs.bitcoincashnode.org/2020-11-15-asert/
QUB uses 60-second spacing, a height gate, and median-of-11 schedule clocks.
At activation the previous block's target is carried forward exactly once.
No node, wallet, network, testnet or regtest is used by this module.
"""
from decimal import Decimal, localcontext

ACTIVATION_HEIGHT = 100000
SPACING = 60
HALF_LIFE = 3600
POW_LIMIT_BITS = 0x1D07FFFF
MTP_WINDOW = 11


def target(bits):
    exponent, mantissa = bits >> 24, bits & 0x7fffff
    if bits & 0x800000 or mantissa == 0 or exponent > 32:
        raise ValueError('invalid compact target')
    n = mantissa << (8 * (exponent - 3)) if exponent >= 3 else mantissa >> (8 * (3 - exponent))
    if not 0 < n < 2**256:
        raise ValueError('zero/overflow target')
    return n


def compact(n):
    if not 0 < n < 2**256:
        raise ValueError('invalid target')
    size = (n.bit_length() + 7) // 8
    value = n << (8 * (3 - size)) if size <= 3 else n >> (8 * (size - 3))
    if value & 0x800000:
        value >>= 8
        size += 1
    return size << 24 | value


def trunc_div(n, d):
    if d <= 0:
        raise ValueError('nonpositive denominator')
    return n // d if n >= 0 else -((-n) // d)


def asert(anchor_bits, time_error, half_life=HALF_LIFE, limit_bits=POW_LIMIT_BITS):
    reference, limit = target(anchor_bits), target(limit_bits)
    if reference > limit:
        raise ValueError('anchor exceeds pow limit')
    exponent = trunc_div(time_error * 65536, half_life)
    shifts, fraction = divmod(exponent, 65536)
    if shifts >= 256:
        return limit_bits
    if shifts <= -256:
        return compact(1)
    factor = 65536 + ((195766423245049 * fraction + 971821376 * fraction**2
                       + 5127 * fraction**3 + 2**47) >> 48)
    result = reference * factor
    result = result >> -shifts if shifts < 0 else result << shifts
    result >>= 16
    return compact(max(1, min(limit, result)))


def mtp(headers):
    if not headers:
        raise ValueError('missing clock history')
    times = sorted(h['time'] for h in headers[-MTP_WINDOW:])
    return times[len(times) // 2]


def v2(headers):
    if len(headers) < 21:
        raise ValueError('v2 fixture requires 21 headers')
    span = max(300, min(4800, headers[-1]['time'] - headers[-21]['time']))
    n = target(headers[-1]['bits']) * span // 1200
    return compact(max(1, min(target(POW_LIMIT_BITS), n)))


def anchored(headers, anchor_index, half_life=HALF_LIFE):
    if not 10 <= anchor_index < len(headers):
        raise ValueError('missing anchor or clock history')
    elapsed = mtp(headers) - mtp(headers[:anchor_index + 1])
    height_delta = len(headers) - 1 - anchor_index
    return asert(headers[anchor_index]['bits'], elapsed - SPACING * height_delta, half_life)


def decimal_oracle_target(anchor_bits, time_error, half_life=HALF_LIFE):
    """Independent ideal exponential, used only to bound polynomial error."""
    with localcontext() as context:
        context.prec = 80
        result = Decimal(target(anchor_bits)) * (Decimal(2) ** (Decimal(time_error) / half_life))
        return max(1, min(target(POW_LIMIT_BITS), int(result)))
