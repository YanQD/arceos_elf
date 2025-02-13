#include <stdio.h>
#include <unistd.h>
#include <sys/types.h>

int main() {
    pid_t pid;

    printf("Before fork\n");

    pid = fork();

    printf("After fork\n");

    if (pid < 0) {
        printf("Fork failed\n");
        return 1;
    }
    else if (pid == 0) {
        printf("This is Child process\n");
    }
    else {
        printf("This is Parent process\n"); 
    }

    return 0;
}