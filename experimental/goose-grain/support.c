// Untimed platform support only: SHA-256, binary64 input, retained RSS.
#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <CommonCrypto/CommonDigest.h>
typedef struct { const uint8_t *data; int64_t len; } Slice;
bool grain_sha256(Slice data, Slice expected) {
    CC_SHA256_CTX c; unsigned char digest[32]; CC_SHA256_Init(&c);
    int64_t offset=0;
    while(offset<data.len) {uint32_t n=(uint32_t)((data.len-offset)>1048576?1048576:data.len-offset); CC_SHA256_Update(&c,data.data+offset,n);offset+=n;}
    CC_SHA256_Final(digest,&c); const char *hex="0123456789abcdef";
    if(expected.len!=64) return false;
    for(int i=0;i<32;i++) if(expected.data[i*2]!=hex[digest[i]>>4] || expected.data[i*2+1]!=hex[digest[i]&15]) return false;
    return true;
}
double grain_read_f64(Slice b, int64_t i) {double x;memcpy(&x,b.data+i*8,8);return x;}

uint64_t grain_checksum(Slice data) {
    CC_SHA256_CTX c; unsigned char digest[32]; CC_SHA256_Init(&c);
    int64_t offset=0;
    while(offset<data.len) {uint32_t n=(uint32_t)((data.len-offset)>1048576?1048576:data.len-offset); CC_SHA256_Update(&c,data.data+offset,n);offset+=n;}
    CC_SHA256_Final(digest,&c);uint64_t result;memcpy(&result,digest,8);return result;
}
