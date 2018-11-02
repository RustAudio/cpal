#pragma once
#include "asiodrivers.h"
#include "asio.h"

// Helper function to wrap confusing preprocessor
extern "C" ASIOError get_sample_rate(double * rate);

// Helper function to wrap confusing preprocessor
extern "C" ASIOError set_sample_rate(double rate);

// Helper function to wrap confusing preprocessor
extern "C" ASIOError can_sample_rate(double rate);

extern "C" bool load_asio_driver(char * name);
extern "C" void remove_current_driver();
extern "C" long get_driver_names(char **names, long maxDrivers);