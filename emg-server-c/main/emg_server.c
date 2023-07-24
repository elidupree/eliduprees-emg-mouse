#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/event_groups.h"
#include "freertos/semphr.h"
#include "esp_system.h"
#include "esp_random.h"
#include "esp_log.h"
#include "nvs_flash.h"
#include "esp_bt.h"
#include "sdkconfig.h"
#include "driver/adc.h"
#include "driver/i2s.h"
//#include "driver/uart.h"
#include <soc/i2s_struct.h>
#include <soc/i2s_reg.h>
#include <soc/rtc.h>

#include "driver/gpio.h"
#include "esp_adc_cal.h"

#include "emg_server.h"

//
//#define TIMES              256
//#define GET_UNIT(x)        ((x>>3) & 0x1)
//
//#define ADC_RESULT_BYTE     2
//#define ADC_CONV_LIMIT_EN   1                       //For ESP32, this should always be set to 1
//#define ADC_CONV_MODE       ADC_CONV_SINGLE_UNIT_1  //ESP32 only supports ADC1 DMA mode
//#define ADC_OUTPUT_TYPE     ADC_DIGI_OUTPUT_FORMAT_TYPE1
//
//static uint16_t adc1_chan_mask = BIT(4) | BIT(5) | BIT(6) | BIT(7);
//#define CHANNEL_LIST_SIZE   12
//static adc_channel_t channel[CHANNEL_LIST_SIZE] = {
//  ADC1_CHANNEL_4, ADC1_CHANNEL_5, ADC1_CHANNEL_6, ADC1_CHANNEL_7,
//  ADC1_CHANNEL_6, ADC1_CHANNEL_5, ADC1_CHANNEL_4, ADC1_CHANNEL_6,
//  ADC1_CHANNEL_4, ADC1_CHANNEL_7, ADC1_CHANNEL_5, ADC1_CHANNEL_7,
//};
//
//static const char *TAG = "ADC DMA";
//
//static void continuous_adc_init()
//{
//    adc_digi_init_config_t adc_dma_config = {
//        .max_store_buf_size = 110024,
//        .conv_num_each_intr = CHANNEL_LIST_SIZE*2, //TIMES,
//        .adc1_chan_mask = adc1_chan_mask,
//        .adc2_chan_mask = 0,
//    };
//    ESP_ERROR_CHECK(adc_digi_initialize(&adc_dma_config));
//
//    adc_digi_configuration_t dig_cfg = {
//        .conv_limit_en = ADC_CONV_LIMIT_EN,
//        .conv_limit_num = 4,//255,
//        .sample_freq_hz = 2 * 1000,
//        .conv_mode = ADC_CONV_MODE,
//        .format = ADC_OUTPUT_TYPE,
//    };
//
//    adc_digi_pattern_config_t adc_pattern[SOC_ADC_PATT_LEN_MAX] = {0};
//    dig_cfg.pattern_num = CHANNEL_LIST_SIZE;
//    for (int i = 0; i < CHANNEL_LIST_SIZE; i++) {
//        uint8_t unit = GET_UNIT(channel[i]);
//        uint8_t ch = channel[i] & 0x7;
//        adc_pattern[i].atten = ADC_ATTEN_DB_11;
//        adc_pattern[i].channel = ch;
//        adc_pattern[i].unit = unit;
//        adc_pattern[i].bit_width = SOC_ADC_DIGI_MAX_BITWIDTH;
//
//        ESP_LOGI(TAG, "adc_pattern[%d].atten is :%x", i, adc_pattern[i].atten);
//        ESP_LOGI(TAG, "adc_pattern[%d].channel is :%x", i, adc_pattern[i].channel);
//        ESP_LOGI(TAG, "adc_pattern[%d].unit is :%x", i, adc_pattern[i].unit);
//    }
//    dig_cfg.adc_pattern = adc_pattern;
//    ESP_ERROR_CHECK(adc_digi_controller_configure(&dig_cfg));
//}


#define SEND_BUFFER_SIZE 1024
volatile uint16_t send_buffer[SEND_BUFFER_SIZE];
const uint16_t SEND_BUFFER_UNUSED = 0xffff;
const uint16_t HEADER_SIZE = 26;

void communication_task(void* arg) {
  uint64_t sample_index = 0;
  uint32_t send_buffer_read_pos = 0;
  const uint16_t MAX_MESSAGE_SIZE = 16 + 82*6;
  uint8_t message_data[MAX_MESSAGE_SIZE];

  uint16_t latest_samples[4];

  // first 8 bytes are a unique delimiter
  memcpy(message_data, "emg_data", 8);
  // next 8 bytes are a random id that disambiguates which run of the server
  esp_fill_random(message_data+8, 8);

  uint16_t message_size = HEADER_SIZE;
  // next 8 bytes are the index of the first sample in the notification, and next 2 are reserved for the number of samples
  *((uint64_t*)&message_data[16]) = sample_index;
  while(1) {
    while (message_size+6 <= MAX_MESSAGE_SIZE) {
      uint16_t average = send_buffer[send_buffer_read_pos];
      if (average == SEND_BUFFER_UNUSED) {
        if (send_buffer_read_pos % 8 == 0) {
          break;
        }
        else {
          continue;
        }
      }
      latest_samples[send_buffer_read_pos % 4] = average;
      send_buffer[send_buffer_read_pos++] = SEND_BUFFER_UNUSED;
      if (send_buffer_read_pos >= SEND_BUFFER_SIZE) {
        send_buffer_read_pos = 0;
      }

      if (send_buffer_read_pos % 4 == 0) {
        message_data[message_size++] = latest_samples[0] >> 4;
        message_data[message_size++] = latest_samples[1] >> 4;
        message_data[message_size++] = latest_samples[2] >> 4;
        message_data[message_size++] = latest_samples[3] >> 4;
        message_data[message_size++] = ((latest_samples[0] & 0xf) << 4) + (latest_samples[1] & 0xf);
        message_data[message_size++] = ((latest_samples[2] & 0xf) << 4) + (latest_samples[3] & 0xf);
        sample_index += 1;
      }
    }

    if (message_size > HEADER_SIZE) {
      *((uint16_t*)&message_data[24]) = message_size;
      //uart_write_bytes(UART_NUM_0, message_data, message_size);
      write(1, message_data, message_size);
      fflush(stdout);
      message_size = HEADER_SIZE;
      *((uint64_t*)&message_data[16]) = sample_index;
    }
    vTaskDelay(1);
  }
}

void adc_task(void * arg) {

//    esp_err_t ret;

//    uint32_t ret_num = 0;
//    uint8_t result[TIMES] = {0};
//    uint32_t n = 0;
//    uint64_t total = 0;
//    memset(result, 0xcc, TIMES);

//    uint32_t totals[4] = {0,0,0,0};
//    uint32_t totals_contributors = 0;
//    uint32_t samples_per_report = 100;
//    uint32_t report_num = 0;
//    uint32_t reports_per_send = 6;


    uint64_t send_buffer_write_pos = 0;

    const adc_atten_t atten = ADC_ATTEN_DB_11;
    adc1_config_width(ADC_WIDTH_BIT_12);
    const adc_channel_t adc_channels[4] = {ADC_CHANNEL_4, ADC_CHANNEL_5, ADC_CHANNEL_6, ADC_CHANNEL_7};
    for(uint32_t adc_index = 0; adc_index < 4; adc_index++) {
      adc1_config_channel_atten(adc_channels[adc_index], atten);
    }

    //Characterize ADC
    esp_adc_cal_characteristics_t *adc_chars;
    adc_chars = calloc(1, sizeof(esp_adc_cal_characteristics_t));
    esp_adc_cal_characterize(ADC_UNIT_1, atten, ADC_WIDTH_BIT_12, 1100, adc_chars);

    //Continuously sample ADC1
//        vTaskDelay(100);
//    int64_t worst_duration = 0;
    const int64_t start_time = esp_timer_get_time();
//    int vals[10] = {0,0,0,0,0,0,0,0,0,0};
    for(uint64_t sample_index = 0; ; sample_index++) {
        for(uint32_t adc_index = 0; adc_index < 4; adc_index++) {
          uint32_t total = 0;
          uint32_t count = 0;
          int64_t start_us = start_time + sample_index * 1000 + adc_index * 250;
          int64_t stop_us = start_us + 200;
          adc_channel_t channel = adc_channels[adc_index];
          while (esp_timer_get_time() < start_us){}

          // with just `while (esp_timer_get_time() < stop_us)`, this usually does 4 samples,
          // but about 12% of the time, does 5 samples; force "<= 4" for consistency.
          // we still want to be able to stop early to allow catch-up
          // (which almost-never happens, although it will happen if you stick in logging code);
          // but always force at least one sample so that we don't have to worry about what to do about divide-by-0
          do {
            total += adc1_get_raw(channel);
            count += 1;
          } while (count < 4 && esp_timer_get_time() < stop_us);
//          if (count < 10) {
//            vals[count] += 1;
//          }

          uint16_t average = total / count;
          uint32_t voltage = esp_adc_cal_raw_to_voltage(average, adc_chars);
          send_buffer[send_buffer_write_pos++] = voltage;
          if (send_buffer_write_pos >= SEND_BUFFER_SIZE) {
            send_buffer_write_pos = 0;
          }
        }
//        if ((sample_index % 1000) == 999) {
//          printf("counts: ");
//          for(uint32_t i = 0; i < 10; i++) {
//            printf("%d, ", vals[i]);
//          }
//          printf("\n");
//        }
//
//        uint32_t adc_reading = 0;
//        //Multisampling
//        const int64_t before_read = esp_timer_get_time();
//        for (int i = 0; i < 1000/12; ++i) {
//          adc_reading =
//          adc_reading = adc1_get_raw(ADC_CHANNEL_5);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_6);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_7);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_6);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_5);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_4);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_6);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_4);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_7);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_5);
//          adc_reading = adc1_get_raw(ADC_CHANNEL_7);
//        }
//        const int64_t after_read = esp_timer_get_time();
//        //Convert adc_reading to voltage in mV
//        int64_t duration = after_read - before_read;
//        //if (duration > worst_duration) {
//        //  worst_duration = duration;
//          printf("duration: %lld\n", duration);
//
//        //}
//        //uint32_t voltage = esp_adc_cal_raw_to_voltage(adc_reading, adc_chars);
//        //printf("Raw: %d\tVoltage: %dmV\n", adc_reading, voltage);
//        vTaskDelay(100);
//        const int64_t after_delay = esp_timer_get_time();
//        int64_t dduration = after_delay - after_read;
//          printf("delay duration: %lld\n", dduration);
    }

//    while(1) {
//        ret = adc_digi_read_bytes(result, TIMES, &ret_num, ADC_MAX_DELAY);
//        if (ret == ESP_OK || ret == ESP_ERR_INVALID_STATE) {
//            if (ret == ESP_ERR_INVALID_STATE) {
////                adc_digi_stop();
////                ret = adc_digi_deinitialize();
////                assert(ret == ESP_OK);
////                continuous_adc_init();
////                adc_digi_start();
//            }
//
//            n += 1;
//            total += ret_num;
//
//            if (n % 10000 == 0) {
//              ESP_LOGI("TASK:", "ret is %x, ret_num is %d, total is %llu, avg is %lld/s", ret, ret_num, total, total*1000000/(esp_timer_get_time() - start_time));
//            }
//
//            for (int i = 0; i < ret_num; i += ADC_RESULT_BYTE) {
//              adc_digi_output_data_t *p = (void*)&result[i];
//              totals[p->type1.channel - 4] += p->type1.data;
//              totals_contributors++;
//              if (totals_contributors == samples_per_report*CHANNEL_LIST_SIZE) {
//                for (int j = 0; j < 4; j++) {
//                  uint16_t average = totals[j] / samples_per_report;
//                  send_buffer[send_buffer_write_pos++] = average;
//                  if (send_buffer_write_pos >= SEND_BUFFER_SIZE) {
//                    send_buffer_write_pos = 0;
//                  }
////                  message_data[(report_num % reports_per_send)*8 + j*2] = average >> 4;
////                  message_data[(report_num % reports_per_send)*8 + j*2 + 1] = average & 0xff;
//                  totals[j] = 0;
//                }
//                totals_contributors = 0;
////                report_num++;
//                if (report_num % reports_per_send == 0) {
////                  esp_ble_gatts_send_indicate(ghack, hack_conn_id, heart_rate_handle_table[IDX_CHAR_VAL_A],
////                                                reports_per_send*8, message_data, false);
//                }
//              }
//            }
//
//            //See `note 1`
//            if (ret_num < TIMES) vTaskDelay(1);
//        } else if (ret == ESP_ERR_TIMEOUT) {
//            /**
//             * ``ESP_ERR_TIMEOUT``: If ADC conversion is not finished until Timeout, you'll get this return error.
//             * Here we set Timeout ``portMAX_DELAY``, so you'll never reach this branch.
//             */
//            ESP_LOGW(TAG, "No data, increase timeout or reduce conv_num_each_intr");
//            vTaskDelay(1000);
//        }
//
//    }
//
//    adc_digi_stop();
//    ret = adc_digi_deinitialize();
//    assert(ret == ESP_OK);
}


void app_main(void)
{

    esp_err_t ret;

    /* Initialize NVS. */
    ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ret = nvs_flash_init();
    }
    ESP_ERROR_CHECK( ret );
//    int j = 0;
//    while(true) {
//      j++;
//      uint8_t message_data[50];
//      for (int i = 0; i < sizeof(message_data); ++i)
//      {
//        message_data[i] = (i+j) % 0xff;
//      }
//      esp_ble_gatts_send_indicate(ghack, hack_conn_id, heart_rate_handle_table[IDX_CHAR_VAL_A],
//                              sizeof(message_data), message_data, false);
//      if ((j % 100) == 0) {vTaskDelay(1);}
//    }

    //continuous_adc_init();
//    int d=8e7/(I2S0.clkm_conf.clkm_div_num+I2S0.clkm_conf.clkm_div_b/I2S0.clkm_conf.clkm_div_a)/2000+0.5;
//    SET_PERI_REG_BITS(I2S_SAMPLE_RATE_CONF_REG(0), I2S_RX_BCK_DIV_NUM, d, I2S_RX_BCK_DIV_NUM_S);
    //adc_digi_start();

    for (uint16_t i = 0; i < SEND_BUFFER_SIZE; ++i) {
      send_buffer[i] = SEND_BUFFER_UNUSED;
    }


    xTaskCreatePinnedToCore(adc_task, "AdcTask", 2*1024, NULL, 5, NULL, 1);
    xTaskCreatePinnedToCore(communication_task, "ComTask", 3*1024, NULL, 5, NULL, 0);

    //i2s_set_sample_rates(I2S_NUM_0, 2000);
//    i2s_stop(0);
//       rtc_clk_apll_enable(true);
//        I2S0.clkm_conf.clkm_div_num = 40;
//        I2S0.clkm_conf.clka_en = 1;  //enable APLL
//        I2S0.clkm_conf.clkm_div_b = 0;
//        I2S0.clkm_conf.clkm_div_a = 1;
//        I2S0.sample_rate_conf.tx_bck_div_num = 40;
//        I2S0.sample_rate_conf.rx_bck_div_num = 40;
//        i2s_start(0);
}
