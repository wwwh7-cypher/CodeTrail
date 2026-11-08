#include "service.h"

static int call_count;

int service_add(int left, int right) {
    call_count++;
    return left + right;
}

void service_reset(void) {
    call_count = 0;
}
