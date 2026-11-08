#include <stdio.h>

#include "service.h"

int main(void) {
    printf("%d\n", service_add(1, 2));
    service_reset();
    return 0;
}
